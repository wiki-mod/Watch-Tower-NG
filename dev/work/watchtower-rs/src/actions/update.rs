#![forbid(unsafe_code)]

use crate::container::Container;
use crate::lifecycle::{self, HookOutcome, LifecycleClient};
use crate::session::Progress;
use crate::sorter::sort_by_dependencies;
use crate::types::{ContainerID, ImageID, Report, RuntimeContainer, UpdateParams};
use crate::rand_name::rand_name;
use std::collections::HashSet;
use std::time::Duration;
use tracing::{debug, error, info, trace};

/// Docker-facing update surface used by the translated legacy update flow.
pub trait UpdateClient: LifecycleClient {
    fn is_container_stale(
        &self,
        container: &Container,
        params: &UpdateParams,
    ) -> std::result::Result<(bool, ImageID), Self::Error>;

    fn stop_container(
        &self,
        container: &Container,
        timeout: Duration,
    ) -> std::result::Result<(), Self::Error>;

    fn start_container(&self, container: &Container) -> std::result::Result<ContainerID, Self::Error>;

    fn rename_container(
        &self,
        container: &Container,
        new_name: &str,
    ) -> std::result::Result<(), Self::Error>;

    fn remove_image_by_id(&self, image_id: &ImageID) -> std::result::Result<(), Self::Error>;
}

/// Update orchestration translated from the legacy Go action flow.
pub fn update<C>(client: &C, params: &UpdateParams) -> std::result::Result<Report, String>
where
    C: UpdateClient,
{
    debug!("Checking containers for updated images");

    let mut progress = Progress::default();

    if params.lifecycle_hooks {
        lifecycle::execute_pre_checks(client, params)
            .map_err(|_| "pre-check execution failed".to_string())?;
    }

    let mut containers = client
        .list_containers()
        .map_err(|_| "container listing failed".to_string())?;
    containers.retain(|container| params.matches(container));

    let containers = scan_and_mark_stale(containers, client, params, &mut progress)?;
    let mut containers = sort_by_dependencies(&containers).map_err(|error| error.to_string())?;
    update_implicit_restart(&mut containers);

    let to_update = select_containers_to_update(&containers, params, &mut progress);
    execute_update(&to_update, client, params, &mut progress);

    if params.lifecycle_hooks {
        lifecycle::execute_post_checks(client, params)
            .map_err(|_| "post-check execution failed".to_string())?;
    }

    Ok(progress.report())
}

fn scan_and_mark_stale(
    mut containers: Vec<Container>,
    client: &impl UpdateClient,
    params: &UpdateParams,
    progress: &mut Progress,
) -> std::result::Result<Vec<Container>, String> {
    for container in &mut containers {
        let stale_result = client
            .is_container_stale(container, params)
            .map_err(|_| "stale check failed".to_string());

        let (stale, newest_image, error) = match stale_result {
            Ok((stale, newest_image)) => (stale, newest_image, None),
            Err(error) => (false, container.image_id().clone(), Some(error)),
        };

        let should_update = stale && !params.no_restart && !container.is_monitor_only(params);
        if error.is_none() && should_update {
            if let Err(config_error) = container.verify_configuration() {
                if tracing::enabled!(tracing::Level::TRACE) {
                    trace!("Image info: {:?}", container.image_info());
                    trace!("Container info: {:?}", container.container_info());
                    if let Some(image_info) = container.image_info() {
                        trace!("Image config: {:?}", image_info.config);
                    }
                }

                let error = config_error.to_string();
                info!("Unable to update container {:?}: {}. Proceeding to next.", container.name(), error);
                progress.add_skipped(container, error);
                container.set_stale(false);
                continue;
            }
        }

        if let Some(error) = error {
            info!(
                "Unable to update container {:?}: {}. Proceeding to next.",
                container.name(),
                error
            );
            container.set_stale(false);
            progress.add_skipped(container, error);
        } else {
            progress.add_scanned(container, newest_image);
            container.set_stale(stale);
        }
    }

    Ok(containers)
}

fn select_containers_to_update(
    containers: &[Container],
    params: &UpdateParams,
    progress: &mut Progress,
) -> Vec<Container> {
    let mut res = Vec::new();

    for container in containers {
        if !container.is_monitor_only(params) {
            res.push(container.clone());
            progress.mark_for_update(container.id());
        }
    }

    res
}

fn execute_update(
    containers_to_update: &[Container],
    client: &impl UpdateClient,
    params: &UpdateParams,
    progress: &mut Progress,
) {
    if params.rolling_restart {
        progress.update_failed(perform_rolling_restart(containers_to_update, client, params));
        return;
    }

    let (failed_stop, stopped_images) = stop_containers_in_reversed_order(containers_to_update, client, params);
    progress.update_failed(failed_stop);
    let failed_start = restart_containers_in_sorted_order(containers_to_update, client, params, stopped_images);
    progress.update_failed(failed_start);
}

fn perform_rolling_restart(
    containers: &[Container],
    client: &impl UpdateClient,
    params: &UpdateParams,
) -> Vec<(ContainerID, String)> {
    let mut cleanup_image_ids = HashSet::<ImageID>::new();
    let mut failed = Vec::with_capacity(containers.len());

    for container in containers.iter().rev() {
        if !container.to_restart() {
            continue;
        }

        match stop_stale_container(container, client, params) {
            Ok(()) => match restart_stale_container(container, client, params) {
                Ok(()) => {
                    if container.is_stale() {
                        cleanup_image_ids.insert(container.image_id().clone());
                    }
                }
                Err(error) => failed.push((container.id().clone(), error)),
            },
            Err(error) => failed.push((container.id().clone(), error)),
        }
    }

    if params.cleanup {
        cleanup_images(client, cleanup_image_ids);
    }

    failed
}

fn stop_containers_in_reversed_order(
    containers: &[Container],
    client: &impl UpdateClient,
    params: &UpdateParams,
) -> (Vec<(ContainerID, String)>, Vec<ImageID>) {
    let mut failed = Vec::with_capacity(containers.len());
    let mut stopped = Vec::with_capacity(containers.len());

    for container in containers.iter().rev() {
        match stop_stale_container(container, client, params) {
            Ok(()) => stopped.push(safe_image_id_or_empty(container)),
            Err(error) => failed.push((container.id().clone(), error)),
        }
    }

    (failed, stopped)
}

fn stop_stale_container(
    container: &Container,
    client: &impl UpdateClient,
    params: &UpdateParams,
) -> std::result::Result<(), String> {
    if container.is_watchtower() {
        debug!("This is the watchtower container {}", container.name());
        return Ok(());
    }

    if !container.to_restart() {
        return Ok(());
    }

    if container.is_linked_to_restarting() {
        let mut verify_container = container.clone();
        verify_container.verify_configuration().map_err(|error| error.to_string())?;
    }

    if params.lifecycle_hooks {
        match lifecycle::execute_pre_update_command(client, container) {
            Ok(HookOutcome::Executed { skip_update: true }) => {
                debug!("Skipping container as the pre-update command returned exit code 75 (EX_TEMPFAIL)");
                return Err("skipping container as the pre-update command returned exit code 75 (EX_TEMPFAIL)".to_string());
            }
            Ok(HookOutcome::Executed { skip_update: false }) | Ok(HookOutcome::Skipped(_)) => {}
            Err(_error) => {
                error!("pre-update execution failed");
                info!("Skipping container as the pre-update command failed");
                return Err("pre-update execution failed".to_string());
            }
        }
    }

    client
        .stop_container(container, params.timeout)
        .map_err(|_| {
            error!("stop container failed");
            "stop container failed".to_string()
        })
}

fn restart_containers_in_sorted_order(
    containers: &[Container],
    client: &impl UpdateClient,
    params: &UpdateParams,
    stopped_images: Vec<ImageID>,
) -> Vec<(ContainerID, String)> {
    let mut cleanup_image_ids = HashSet::<ImageID>::new();
    let mut failed = Vec::with_capacity(containers.len());
    let stopped: HashSet<ImageID> = stopped_images.into_iter().collect();

    for container in containers {
        let safe_image_id = safe_image_id_or_empty(container);
        if !container.to_restart() || !stopped.contains(&safe_image_id) {
            continue;
        }

        match restart_stale_container(container, client, params) {
            Ok(()) => {
                if container.is_stale() {
                    cleanup_image_ids.insert(container.image_id().clone());
                }
            }
            Err(error) => failed.push((container.id().clone(), error)),
        }
    }

    if params.cleanup {
        cleanup_images(client, cleanup_image_ids);
    }

    failed
}

fn cleanup_images(client: &impl UpdateClient, image_ids: HashSet<ImageID>) {
    for image_id in image_ids {
        if image_id.as_str().is_empty() {
            continue;
        }

        if client.remove_image_by_id(&image_id).is_err() {
            error!("remove image failed");
        }
    }
}

fn restart_stale_container(
    container: &Container,
    client: &impl UpdateClient,
    params: &UpdateParams,
) -> std::result::Result<(), String> {
    if container.is_watchtower() && client.rename_container(container, &rand_name()).is_err() {
        error!("rename container failed");
        return Ok(());
    }

    if !params.no_restart {
        let new_container_id = client
            .start_container(container)
            .map_err(|_| {
                error!("start container failed");
                "start container failed".to_string()
            })?;

        if container.to_restart() && params.lifecycle_hooks {
            let _ = lifecycle::execute_post_update_command(client, &new_container_id);
        }
    }

    Ok(())
}

fn update_implicit_restart<C: RuntimeContainer>(containers: &mut [C]) {
    for ci in 0..containers.len() {
        if containers[ci].to_restart() {
            continue;
        }

        if let Some(link) = linked_container_marked_for_restart(containers[ci].links(), containers) {
            debug!(restarting = link, linked = containers[ci].name(), "container is linked to restarting");
            containers[ci].set_linked_to_restarting(true);
        }
    }
}

fn linked_container_marked_for_restart(links: &[String], containers: &[impl RuntimeContainer]) -> Option<String> {
    for link_name in links {
        for candidate in containers {
            if candidate.name() == link_name && candidate.to_restart() {
                return Some(link_name.clone());
            }
        }
    }

    None
}

fn safe_image_id_or_empty(container: &Container) -> ImageID {
    container
        .safe_image_id()
        .cloned()
        .unwrap_or_else(|| ImageID::new(""))
}
