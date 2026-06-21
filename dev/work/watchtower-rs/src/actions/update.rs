#![forbid(unsafe_code)]

use crate::container::Container;
use crate::lifecycle::{self, HookOutcome, LifecycleClient};
use crate::rand_name::rand_name;
use crate::session::Progress;
use crate::sorter::sort_by_dependencies;
use crate::types::{ContainerID, ImageID, Report, UpdateParams};
use std::collections::HashMap;
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

    fn start_container(
        &self,
        container: &Container,
    ) -> std::result::Result<ContainerID, Self::Error>;

    fn rename_container(
        &self,
        container: &Container,
        new_name: &str,
    ) -> std::result::Result<(), Self::Error>;

    fn remove_image_by_id(&self, image_id: &ImageID) -> std::result::Result<(), Self::Error>;
}

/// Update orchestration translated from the legacy Go action flow.
pub fn update<C>(client: &C, params: &UpdateParams) -> std::result::Result<Report, C::Error>
where
    C: UpdateClient,
    C::Error: std::fmt::Display + From<String>,
{
    debug!("Checking containers for updated images");

    let mut progress = Progress::default();

    if params.lifecycle_hooks {
        lifecycle::execute_pre_checks(client, params)?;
    }

    let mut containers = client.list_containers()?;
    containers.retain(|container| params.matches(container));

    let containers = scan_and_mark_stale(containers, client, params, &mut progress)?;
    let mut containers = sort_by_dependencies(&containers)?;
    update_implicit_restart(&mut containers);

    let mut to_update = select_containers_to_update(&containers, params, &mut progress);
    execute_update(&mut to_update, client, params, &mut progress);

    if params.lifecycle_hooks {
        lifecycle::execute_post_checks(client, params)?;
    }

    Ok(progress.report())
}

fn scan_and_mark_stale<C>(
    mut containers: Vec<Container>,
    client: &C,
    params: &UpdateParams,
    progress: &mut Progress,
) -> std::result::Result<Vec<Container>, C::Error>
where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    for i in 0..containers.len() {
        let stale_result = client.is_container_stale(&containers[i], params);

        let (stale, newest_image) = match stale_result {
            Ok((stale, newest_image)) => (stale, newest_image),
            Err(err) => {
                info!(
                    "Unable to update container {:?}: {}. Proceeding to next.",
                    containers[i].name(),
                    err
                );
                progress.add_skipped(&containers[i], err.to_string());
                containers[i].set_stale(false);
                continue;
            }
        };

        let should_update = stale && !params.no_restart && !containers[i].is_monitor_only(params);

        if should_update {
            if let Err(config_err) = containers[i].verify_configuration() {
                if tracing::enabled!(tracing::Level::TRACE) {
                    if let Some(image_info) = containers[i].image_info() {
                        trace!("Image info: {:?}", image_info);
                        trace!("Image config: {:?}", image_info.config);
                    }
                    trace!("Container info: {:?}", containers[i].container_info());
                }

                info!(
                    "Unable to update container {:?}: {}. Proceeding to next.",
                    containers[i].name(),
                    config_err
                );
                progress.add_skipped(&containers[i], config_err.to_string());
                containers[i].set_stale(false);
                continue;
            }
        }

        progress.add_scanned(&containers[i], newest_image);
        containers[i].set_stale(stale);
    }

    Ok(containers)
}

fn select_containers_to_update(
    containers: &[Container],
    params: &UpdateParams,
    progress: &mut Progress,
) -> Vec<Container> {
    let mut res = Vec::new();

    for c in containers {
        if !c.is_monitor_only(params) {
            res.push(c.clone());
            progress.mark_for_update(c.id());
        }
    }

    res
}

fn execute_update<C>(
    containers_to_update: &mut [Container],
    client: &C,
    params: &UpdateParams,
    progress: &mut Progress,
) where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    if params.rolling_restart {
        let failed = perform_rolling_restart(containers_to_update, client, params);
        for (id, err) in failed {
            progress.update_failed(vec![(id, err)]);
        }
        return;
    }

    let (failed_stop, stopped_images) =
        stop_containers_in_reversed_order(containers_to_update, client, params);
    progress.update_failed(failed_stop);

    let failed_start =
        restart_containers_in_sorted_order(containers_to_update, client, params, &stopped_images);
    progress.update_failed(failed_start);
}

fn perform_rolling_restart<C>(
    containers: &mut [Container],
    client: &C,
    params: &UpdateParams,
) -> Vec<(ContainerID, String)>
where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    let mut cleanup_image_ids: HashMap<ImageID, bool> = HashMap::new();
    let mut failed: Vec<(ContainerID, String)> = Vec::new();

    for i in (0..containers.len()).rev() {
        if !containers[i].to_restart() {
            continue;
        }

        match stop_stale_container(&mut containers[i], client, params) {
            Ok(()) => match restart_stale_container(&containers[i], client, params) {
                Ok(()) => {
                    if containers[i].is_stale() {
                        cleanup_image_ids.insert(containers[i].image_id().clone(), true);
                    }
                }
                Err(err) => failed.push((containers[i].id().clone(), err)),
            },
            Err(err) => failed.push((containers[i].id().clone(), err)),
        }
    }

    if params.cleanup {
        cleanup_images(client, cleanup_image_ids);
    }

    failed
}

fn stop_containers_in_reversed_order<C>(
    containers: &mut [Container],
    client: &C,
    params: &UpdateParams,
) -> (Vec<(ContainerID, String)>, HashMap<ImageID, bool>)
where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    let mut failed: Vec<(ContainerID, String)> = Vec::new();
    let mut stopped: HashMap<ImageID, bool> = HashMap::new();

    for i in (0..containers.len()).rev() {
        match stop_stale_container(&mut containers[i], client, params) {
            Ok(()) => {
                stopped.insert(safe_image_id_or_empty(&containers[i]), true);
            }
            Err(err) => {
                failed.push((containers[i].id().clone(), err));
            }
        }
    }

    (failed, stopped)
}

fn stop_stale_container<C>(
    container: &mut Container,
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<(), String>
where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    if container.is_watchtower() {
        debug!("This is the watchtower container {}", container.name());
        return Ok(());
    }

    if !container.to_restart() {
        return Ok(());
    }

    if container.is_linked_to_restarting() {
        if let Err(err) = container.verify_configuration() {
            return Err(err.to_string());
        }
    }

    if params.lifecycle_hooks {
        match lifecycle::execute_pre_update_command(client, container) {
            Ok(HookOutcome::Executed { skip_update: true }) => {
                debug!(
                    "Skipping container as the pre-update command returned exit code 75 (EX_TEMPFAIL)"
                );
                return Err(
                    "skipping container as the pre-update command returned exit code 75 (EX_TEMPFAIL)"
                        .to_string(),
                );
            }
            Ok(HookOutcome::Executed { skip_update: false }) | Ok(HookOutcome::Skipped(_)) => {}
            Err(_) => {
                error!("pre-update execution failed");
                info!("Skipping container as the pre-update command failed");
                return Err("pre-update execution failed".to_string());
            }
        }
    }

    client
        .stop_container(container, params.timeout)
        .map_err(|e| {
            error!("{}", e);
            e.to_string()
        })
}

fn restart_containers_in_sorted_order<C>(
    containers: &[Container],
    client: &C,
    params: &UpdateParams,
    stopped_images: &HashMap<ImageID, bool>,
) -> Vec<(ContainerID, String)>
where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    let mut cleanup_image_ids: HashMap<ImageID, bool> = HashMap::new();
    let mut failed: Vec<(ContainerID, String)> = Vec::new();

    for c in containers {
        if !c.to_restart() {
            continue;
        }

        if !stopped_images.contains_key(&safe_image_id_or_empty(c)) {
            continue;
        }

        match restart_stale_container(c, client, params) {
            Ok(()) => {
                if c.is_stale() {
                    cleanup_image_ids.insert(c.image_id().clone(), true);
                }
            }
            Err(err) => failed.push((c.id().clone(), err)),
        }
    }

    if params.cleanup {
        cleanup_images(client, cleanup_image_ids);
    }

    failed
}

fn cleanup_images<C>(client: &C, image_ids: HashMap<ImageID, bool>)
where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    for image_id in image_ids.keys() {
        if image_id.as_str().is_empty() {
            continue;
        }

        if let Err(e) = client.remove_image_by_id(image_id) {
            error!("{}", e);
        }
    }
}

fn restart_stale_container<C>(
    container: &Container,
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<(), String>
where
    C: UpdateClient,
    C::Error: std::fmt::Display,
{
    if container.is_watchtower() {
        if client.rename_container(container, &rand_name()).is_err() {
            error!("rename container failed");
            return Ok(());
        }
    }

    if !params.no_restart {
        let new_container_id = client.start_container(container).map_err(|e| {
            error!("{}", e);
            e.to_string()
        })?;

        if container.to_restart() && params.lifecycle_hooks {
            lifecycle::execute_post_update_command(client, &new_container_id);
        }
    }

    Ok(())
}

/// UpdateImplicitRestart iterates through the passed containers, setting the
/// `LinkedToRestarting` flag if any of its linked containers are marked for restart
fn update_implicit_restart(containers: &mut [Container]) {
    for ci in 0..containers.len() {
        if containers[ci].to_restart() {
            continue;
        }

        if let Some(link) = linked_container_marked_for_restart(containers[ci].links(), containers)
        {
            debug!(
                restarting = link,
                linked = containers[ci].name(),
                "container is linked to restarting"
            );
            containers[ci].set_linked_to_restarting(true);
        }
    }
}

/// linkedContainerMarkedForRestart returns the name of the first link that matches a
/// container marked for restart
fn linked_container_marked_for_restart(
    links: &[String],
    containers: &[Container],
) -> Option<String> {
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
