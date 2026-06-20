#![forbid(unsafe_code)]

//! Pure runtime action helpers translated from the legacy Go implementation.
//!
//! This module intentionally stays Docker-agnostic. It only encodes the
//! container-ordering and restart decisions that can be derived from in-memory
//! state.

use crate::sorter::{sort_by_created_at, sort_by_dependencies, SortableContainer};
use crate::session::Progress;
use crate::types::{ContainerID, RuntimeContainer, UpdateParams};
use crate::types::ImageID;
use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Update orchestration derived from the legacy Go action flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatePlan {
    /// Container IDs in dependency order.
    pub sorted_container_ids: Vec<ContainerID>,
    /// Container IDs selected for update.
    pub update_container_ids: Vec<ContainerID>,
    /// Container IDs that would be stopped first, in reverse dependency order.
    pub stop_order: Vec<ContainerID>,
    /// Container IDs that would be restarted after stopping, in dependency order.
    pub restart_order: Vec<ContainerID>,
    /// Cleanup candidates after duplicate and in-use filtering.
    pub cleanup_image_ids: Vec<ImageID>,
}

/// Cleanup plan for older watchtower instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchtowerInstanceCleanupPlan {
    /// Container IDs that should be stopped, ordered from oldest to newest.
    pub stop_container_ids: Vec<ContainerID>,
    /// Image IDs that should be removed after the stop step.
    pub cleanup_image_ids: Vec<ImageID>,
}

/// Docker-facing update surface used by the Rust orchestration layer.
pub trait UpdateClient: crate::lifecycle::LifecycleClient {
    fn is_container_stale(
        &self,
        container: &crate::container::Container,
        params: &UpdateParams,
    ) -> std::result::Result<(bool, ImageID), Self::Error>;

    fn stop_container(
        &self,
        container: &crate::container::Container,
        timeout: Duration,
    ) -> std::result::Result<(), Self::Error>;

    fn start_container(
        &self,
        container: &crate::container::Container,
    ) -> std::result::Result<ContainerID, Self::Error>;

    fn rename_container(
        &self,
        container: &crate::container::Container,
        new_name: &str,
    ) -> std::result::Result<(), Self::Error>;

    fn remove_image_by_id(&self, image_id: &ImageID) -> std::result::Result<(), Self::Error>;
}

fn random_container_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("watchtower-{nanos:x}")
}

/// Fail when rolling restarts are requested on dependency-linked containers.
pub fn check_for_sanity<C: RuntimeContainer>(containers: &[C], rolling_restarts: bool) -> Result<(), String> {
    if !rolling_restarts {
        return Ok(());
    }

    for container in containers {
        if !container.links().is_empty() {
            return Err(format!(
                "{:?} is depending on at least one other container. This is not compatible with rolling restarts",
                container.name()
            ));
        }
    }

    Ok(())
}

/// Mark containers that depend on a restarting container as linked-to-restarting.
pub fn update_implicit_restart<C: RuntimeContainer>(containers: &mut [C]) {
    for idx in 0..containers.len() {
        if containers[idx].to_restart() {
            continue;
        }

        let links = containers[idx].links().to_vec();
        let linked_restart = links.iter().any(|link_name| {
            containers
                .iter()
                .any(|candidate| candidate.name() == link_name && candidate.to_restart())
        });

        if linked_restart {
            containers[idx].set_linked_to_restarting(true);
        }
    }
}

/// Return the subset of container IDs that should be updated.
pub fn select_containers_to_update<C: RuntimeContainer>(
    containers: &mut [C],
    params: &UpdateParams,
) -> Vec<ContainerID> {
    let mut selected = Vec::new();

    for container in containers.iter_mut() {
        if container.is_monitor_only(params) {
            continue;
        }

        selected.push(container.id().clone());
    }

    selected
}

/// Build the container update plan from the current runtime snapshot.
///
/// This mirrors the Go update flow at a planning level: sort by dependencies,
/// mark implicit restarts, pick update candidates, and derive cleanup images.
pub fn build_update_plan<C>(
    containers: &[C],
    params: &UpdateParams,
) -> Result<UpdatePlan, String>
where
    C: RuntimeContainer + SortableContainer + Clone,
{
    check_for_sanity(containers, params.rolling_restart)?;

    let mut ordered = sort_by_dependencies(containers)?;
    update_implicit_restart(&mut ordered);

    let sorted_container_ids = ordered.iter().map(|container| container.id().clone()).collect();
    let update_container_ids = select_containers_to_update(&mut ordered, params);
    let stop_order = update_container_ids.iter().cloned().rev().collect();
    let restart_order = update_container_ids.clone();

    let cleanup_image_ids = if params.cleanup {
        let stale_image_ids = ordered
            .iter()
            .filter(|container| container.is_stale())
            .map(|container| container.image_id().clone());

        retain_unused_cleanup_image_ids(&ordered, stale_image_ids)
    } else {
        Vec::new()
    };

    Ok(UpdatePlan {
        sorted_container_ids,
        update_container_ids,
        stop_order,
        restart_order,
        cleanup_image_ids,
    })
}

/// Normalize cleanup image IDs before removal.
///
/// The legacy Go cleanup path used a set-like map, which naturally skipped
/// duplicates and ignored empty IDs. This helper preserves that behavior for
/// later cleanup orchestration slices.
pub fn dedupe_cleanup_image_ids(image_ids: impl IntoIterator<Item = ImageID>) -> Vec<ImageID> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();

    for image_id in image_ids {
        if image_id.as_str().is_empty() {
            continue;
        }

        if seen.insert(image_id.as_str().to_string()) {
            unique.push(image_id);
        }
    }

    unique
}

/// Drop cleanup candidates that are still referenced by a live container.
///
/// This protects against the cleanup bug where a shared image could be removed
/// even though another container still depended on it.
pub fn retain_unused_cleanup_image_ids<C: RuntimeContainer>(
    containers: &[C],
    image_ids: impl IntoIterator<Item = ImageID>,
) -> Vec<ImageID> {
    let in_use: HashSet<String> = containers
        .iter()
        .filter(|container| !container.is_stale())
        .map(|container| container.image_id().as_str().to_string())
        .collect();

    dedupe_cleanup_image_ids(image_ids)
        .into_iter()
        .filter(|image_id| !in_use.contains(image_id.as_str()))
        .collect()
}

/// Build the cleanup plan for older watchtower instances.
///
/// The Go implementation sorts by creation time and keeps the newest instance
/// alive. This helper now does the same from the in-memory snapshot instead of
/// trusting the input order, then returns the older instances that should be
/// stopped plus their image IDs when cleanup is enabled.
pub fn build_watchtower_instance_cleanup_plan<C: RuntimeContainer + Clone>(
    containers: &[C],
    cleanup: bool,
) -> WatchtowerInstanceCleanupPlan {
    let ordered = sort_by_created_at(containers);

    let stop_container_ids = ordered
        .iter()
        .take(ordered.len().saturating_sub(1))
        .map(|container| container.id().clone())
        .collect();

    let cleanup_image_ids = if cleanup {
        dedupe_cleanup_image_ids(
            ordered
                .iter()
                .take(ordered.len().saturating_sub(1))
                .map(|container| container.image_id().clone()),
        )
    } else {
        Vec::new()
    };

    WatchtowerInstanceCleanupPlan {
        stop_container_ids,
        cleanup_image_ids,
    }
}

/// Return the cleanup work required when more than one watchtower instance is present.
///
/// The legacy Go path treats zero or one instances as a no-op and only returns
/// cleanup work when older instances need to be stopped.
pub fn check_for_multiple_watchtower_instances<C: RuntimeContainer + Clone>(
    containers: &[C],
    cleanup: bool,
) -> Option<WatchtowerInstanceCleanupPlan> {
    if containers.len() <= 1 {
        return None;
    }

    Some(build_watchtower_instance_cleanup_plan(containers, cleanup))
}

/// Execute the translated update flow against a Docker-facing client.
pub fn update<C>(client: &C, params: &UpdateParams) -> std::result::Result<crate::types::Report, String>
where
    C: UpdateClient,
{
    let mut progress = Progress::default();

    if params.lifecycle_hooks {
        crate::lifecycle::execute_pre_checks(client, params)
            .map_err(|_| "pre-check execution failed".to_string())?;
    }

    let mut containers = client
        .list_containers()
        .map_err(|_| "container listing failed".to_string())?;
    containers.retain(|container| params.matches(container));
    let containers = scan_and_mark_stale(containers, client, params, &mut progress)?;
    let mut containers = sort_by_dependencies(&containers)
        .map_err(|error| format!("dependency sorting failed: {error}"))?;
    update_implicit_restart(&mut containers);

    let to_update = select_containers_to_update(&mut containers, params);
    for container_id in &to_update {
        progress.mark_for_update(container_id);
    }

    execute_update(&mut containers, client, params, &mut progress)?;

    if params.lifecycle_hooks {
        crate::lifecycle::execute_post_checks(client, params)
            .map_err(|_| "post-check execution failed".to_string())?;
    }

    Ok(progress.report())
}

fn scan_and_mark_stale<C>(
    mut containers: Vec<crate::container::Container>,
    client: &C,
    params: &UpdateParams,
    progress: &mut Progress,
) -> std::result::Result<Vec<crate::container::Container>, String>
where
    C: UpdateClient,
{
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
                let error = config_error.to_string();
                progress.add_skipped(container, error.clone());
                container.set_stale(false);
                continue;
            }
        }

        if let Some(error) = error {
            progress.add_skipped(container, error);
            container.set_stale(false);
        } else {
            progress.add_scanned(container, newest_image);
            container.set_stale(stale);
        }
    }

    Ok(containers)
}

fn execute_update<C>(
    containers: &mut [crate::container::Container],
    client: &C,
    params: &UpdateParams,
    progress: &mut Progress,
) -> std::result::Result<(), String>
where
    C: UpdateClient,
{
    if params.rolling_restart {
        let failures = perform_rolling_restart(containers, client, params)?;
        progress.update_failed(failures);
        return Ok(());
    }

    let (failed_stop, stopped_images) = stop_containers_in_reversed_order(containers, client, params)?;
    progress.update_failed(failed_stop);
    let failed_start = restart_containers_in_sorted_order(containers, client, params, stopped_images)?;
    progress.update_failed(failed_start);

    Ok(())
}

fn perform_rolling_restart<C>(
    containers: &mut [crate::container::Container],
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<Vec<(ContainerID, String)>, String>
where
    C: UpdateClient,
{
    let mut cleanup_image_ids = Vec::new();
    let mut failures = Vec::new();

    for container in containers.iter_mut().rev() {
        if !container.to_restart() {
            continue;
        }

        match stop_stale_container(container, client, params) {
            Ok(()) => match restart_stale_container(container, client, params) {
                Ok(()) => {
                    if container.is_stale() {
                        cleanup_image_ids.push(container.image_id().clone());
                    }
                }
                Err(error) => failures.push((container.id().clone(), error)),
            },
            Err(error) => failures.push((container.id().clone(), error)),
        }
    }

    if params.cleanup {
        cleanup_images(client, cleanup_image_ids)?;
    }

    Ok(failures)
}

fn stop_containers_in_reversed_order<C>(
    containers: &mut [crate::container::Container],
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<(Vec<(ContainerID, String)>, Vec<ImageID>), String>
where
    C: UpdateClient,
{
    let mut failed = Vec::new();
    let mut stopped = Vec::new();

    for container in containers.iter_mut().rev() {
        match stop_stale_container(container, client, params) {
            Ok(()) => {
                stopped.push(safe_image_id_or_empty(container));
            }
            Err(error) => failed.push((container.id().clone(), error)),
        }
    }

    Ok((failed, stopped))
}

fn restart_containers_in_sorted_order<C>(
    containers: &mut [crate::container::Container],
    client: &C,
    params: &UpdateParams,
    stopped_images: Vec<ImageID>,
) -> std::result::Result<Vec<(ContainerID, String)>, String>
where
    C: UpdateClient,
{
    let mut failed = Vec::new();
    let stopped: HashSet<ImageID> = stopped_images.into_iter().collect();
    let mut cleanup_image_ids = Vec::new();

    for container in containers.iter_mut() {
        let safe_image_id = safe_image_id_or_empty(container);
        if !container.to_restart() || !stopped.contains(&safe_image_id) {
            continue;
        }

        match restart_stale_container(container, client, params) {
            Ok(()) => {
                if container.is_stale() {
                    cleanup_image_ids.push(container.image_id().clone());
                }
            }
            Err(error) => failed.push((container.id().clone(), error)),
        }
    }

    if params.cleanup {
        cleanup_images(client, cleanup_image_ids)?;
    }

    Ok(failed)
}

fn stop_stale_container<C>(
    container: &crate::container::Container,
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<(), String>
where
    C: UpdateClient,
{
    if container.is_watchtower() {
        return Ok(());
    }

    if !container.to_restart() {
        return Ok(());
    }

    if container.is_linked_to_restarting() {
        let mut verify_container = container.clone();
        verify_container
            .verify_configuration()
            .map_err(|error| error.to_string())?;
    }

    if params.lifecycle_hooks {
        match crate::lifecycle::execute_pre_update_command(client, container)
            .map_err(|_| "pre-update execution failed".to_string())?
        {
            crate::lifecycle::HookOutcome::Executed { skip_update: true } => {
                return Err("skipping container as the pre-update command returned exit code 75 (EX_TEMPFAIL)".to_string())
            }
            crate::lifecycle::HookOutcome::Executed { skip_update: false } => {}
            crate::lifecycle::HookOutcome::Skipped(_) => {}
        }
    }

    client
        .stop_container(container, params.timeout)
        .map_err(|_| "stop container failed".to_string())
}

fn restart_stale_container<C>(
    container: &crate::container::Container,
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<(), String>
where
    C: UpdateClient,
{
    if container.is_watchtower() {
        let new_name = random_container_name();
        client
            .rename_container(container, &new_name)
            .map_err(|_| "rename container failed".to_string())?;
    }

    if !params.no_restart {
        let new_container_id = client
            .start_container(container)
            .map_err(|_| "start container failed".to_string())?;
        if container.to_restart() && params.lifecycle_hooks {
            let _ = crate::lifecycle::execute_post_update_command(client, &new_container_id);
        }
    }

    Ok(())
}

fn cleanup_images<C>(client: &C, image_ids: Vec<ImageID>) -> std::result::Result<(), String>
where
    C: UpdateClient,
{
    for image_id in dedupe_cleanup_image_ids(image_ids) {
        if image_id.as_str().is_empty() {
            continue;
        }

        client
            .remove_image_by_id(&image_id)
            .map_err(|_| "remove image failed".to_string())?;
    }

    Ok(())
}

fn safe_image_id_or_empty(container: &crate::container::Container) -> ImageID {
    container
        .safe_image_id()
        .cloned()
        .unwrap_or_else(|| ImageID::new(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use crate::container::{Container, ContainerConfig, ContainerInspect, ContainerState, HostConfig};

    #[derive(Clone)]
    struct MockContainer {
        id: ContainerID,
        name: String,
        links: Vec<String>,
        image_id: ImageID,
        created_at: String,
        stale: bool,
        linked_to_restarting: bool,
        monitor_only: bool,
    }

    impl RuntimeContainer for MockContainer {
        fn id(&self) -> &ContainerID {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn links(&self) -> &[String] {
            &self.links
        }

        fn image_id(&self) -> &ImageID {
            &self.image_id
        }

        fn created_at(&self) -> &str {
            &self.created_at
        }

        fn is_watchtower(&self) -> bool {
            false
        }

        fn is_stale(&self) -> bool {
            self.stale
        }

        fn set_stale(&mut self, value: bool) {
            self.stale = value;
        }

        fn is_linked_to_restarting(&self) -> bool {
            self.linked_to_restarting
        }

        fn set_linked_to_restarting(&mut self, value: bool) {
            self.linked_to_restarting = value;
        }

        fn is_monitor_only(&self, params: &UpdateParams) -> bool {
            self.monitor_only || params.monitor_only
        }
    }

    impl SortableContainer for MockContainer {
        fn name(&self) -> &str {
            &self.name
        }

        fn links(&self) -> &[String] {
            &self.links
        }
    }

    fn container(id: &str, name: &str, links: &[&str], image_id: &str) -> MockContainer {
        MockContainer {
            id: ContainerID::from(id),
            name: name.to_string(),
            links: links.iter().map(|link| (*link).to_string()).collect(),
            image_id: ImageID::from(image_id),
            created_at: "2024-06-18T12:00:00Z".to_string(),
            stale: false,
            linked_to_restarting: false,
            monitor_only: false,
        }
    }

    fn update_container(id: &str, name: &str) -> Container {
        Container::new(
            ContainerInspect {
                id: ContainerID::from(id),
                name: name.to_string(),
                image: ImageID::from(""),
                created: "2024-06-18T12:00:00Z".to_string(),
                state: ContainerState {
                    running: true,
                    restarting: false,
                },
                config: Some(ContainerConfig::default()),
                host_config: Some(HostConfig::default()),
                network_settings: None,
            },
            None,
        )
    }

    struct MockUpdateClient {
        containers: Vec<Container>,
        stale_results: RefCell<VecDeque<std::result::Result<(bool, ImageID), String>>>,
        stop_calls: RefCell<Vec<ContainerID>>,
        start_calls: RefCell<Vec<ContainerID>>,
    }

    impl MockUpdateClient {
        fn new(
            containers: Vec<Container>,
            stale_results: impl IntoIterator<Item = std::result::Result<(bool, ImageID), String>>,
        ) -> Self {
            Self {
                containers,
                stale_results: RefCell::new(stale_results.into_iter().collect()),
                stop_calls: RefCell::new(Vec::new()),
                start_calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl crate::lifecycle::LifecycleClient for MockUpdateClient {
        type Error = String;

        fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error> {
            Ok(self.containers.clone())
        }

        fn get_container(
            &self,
            _container_id: &ContainerID,
        ) -> std::result::Result<Container, Self::Error> {
            Err("not used".to_string())
        }

        fn execute_command(
            &self,
            _container_id: &ContainerID,
            _command: &str,
            _timeout_minutes: i64,
        ) -> std::result::Result<bool, Self::Error> {
            Err("not used".to_string())
        }
    }

    impl UpdateClient for MockUpdateClient {
        fn is_container_stale(
            &self,
            container: &Container,
            _params: &UpdateParams,
        ) -> std::result::Result<(bool, ImageID), Self::Error> {
            self.stale_results
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok((false, container.image_id().clone())))
        }

        fn stop_container(
            &self,
            container: &Container,
            _timeout: std::time::Duration,
        ) -> std::result::Result<(), Self::Error> {
            self.stop_calls.borrow_mut().push(container.id().clone());
            Ok(())
        }

        fn start_container(
            &self,
            container: &Container,
        ) -> std::result::Result<ContainerID, Self::Error> {
            self.start_calls.borrow_mut().push(container.id().clone());
            Ok(container.id().clone())
        }

        fn rename_container(
            &self,
            _container: &Container,
            _new_name: &str,
        ) -> std::result::Result<(), Self::Error> {
            Ok(())
        }

        fn remove_image_by_id(&self, _image_id: &ImageID) -> std::result::Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn sanity_check_rejects_linked_containers_with_rolling_restart() {
        let containers = vec![container("a", "/alpha", &["/beta"], "sha256:a")];

        let err = check_for_sanity(&containers, true).expect_err("should reject");

        assert!(err.contains("rolling restarts"));
        assert!(err.contains("/alpha"));
    }

    #[test]
    fn implicit_restart_marks_linked_dependents() {
        let mut containers = vec![
            container("a", "/alpha", &[], "sha256:a"),
            container("b", "/beta", &["/alpha"], "sha256:b"),
            container("c", "/gamma", &["/beta"], "sha256:c"),
        ];
        containers[0].stale = true;

        update_implicit_restart(&mut containers);

        assert!(containers[1].linked_to_restarting);
        assert!(containers[2].linked_to_restarting);
    }

    #[test]
    fn select_containers_to_update_skips_monitor_only_entries() {
        let mut containers = vec![
            container("a", "/alpha", &[], "sha256:a"),
            container("b", "/beta", &[], "sha256:b"),
        ];
        containers[1].monitor_only = true;

        let params = UpdateParams::default();
        let selected = select_containers_to_update(&mut containers, &params);

        assert_eq!(selected, vec![ContainerID::from("a")]);
    }

    #[test]
    fn dedupe_cleanup_image_ids_skips_empty_and_duplicate_entries() {
        let image_ids = vec![
            ImageID::from(""),
            ImageID::from("sha256:deadbeef"),
            ImageID::from("sha256:deadbeef"),
            ImageID::from("sha256:beadfeed"),
        ];

        assert_eq!(
            dedupe_cleanup_image_ids(image_ids),
            vec![
                ImageID::from("sha256:deadbeef"),
                ImageID::from("sha256:beadfeed"),
            ]
        );
    }

    #[test]
    fn retain_unused_cleanup_image_ids_skips_images_still_used_by_other_containers() {
        let containers = vec![
            container("a", "/alpha", &[], "sha256:shared"),
            container("b", "/beta", &[], "sha256:fresh"),
        ];

        let candidate_ids = vec![
            ImageID::from("sha256:stale-old"),
            ImageID::from("sha256:shared"),
            ImageID::from("sha256:stale-old"),
            ImageID::from(""),
        ];

        assert_eq!(
            retain_unused_cleanup_image_ids(&containers, candidate_ids),
            vec![ImageID::from("sha256:stale-old")]
        );
    }

    #[test]
    fn build_update_plan_sorts_updates_and_filters_cleanup_candidates() {
        let mut containers = vec![
            container("a", "/alpha", &["/beta"], "sha256:old-alpha"),
            container("b", "/beta", &[], "sha256:shared"),
            container("c", "/gamma", &[], "sha256:stale-gamma"),
        ];

        containers[0].stale = true;
        containers[2].stale = true;

        let params = UpdateParams {
            cleanup: true,
            ..UpdateParams::default()
        };

        let plan = build_update_plan(&containers, &params).expect("plan should build");

        assert_eq!(
            plan.sorted_container_ids,
            vec![
                ContainerID::from("b"),
                ContainerID::from("a"),
                ContainerID::from("c"),
            ]
        );
        assert_eq!(
            plan.update_container_ids,
            vec![
                ContainerID::from("b"),
                ContainerID::from("a"),
                ContainerID::from("c"),
            ]
        );
        assert_eq!(
            plan.stop_order,
            vec![
                ContainerID::from("c"),
                ContainerID::from("a"),
                ContainerID::from("b"),
            ]
        );
        assert_eq!(plan.restart_order, plan.update_container_ids);
        assert_eq!(
            plan.cleanup_image_ids,
            vec![
                ImageID::from("sha256:old-alpha"),
                ImageID::from("sha256:stale-gamma"),
            ]
        );
    }

    #[test]
    fn build_update_plan_rejects_rolling_restarts_for_linked_containers() {
        let containers = vec![container("a", "/alpha", &["/beta"], "sha256:a")];
        let params = UpdateParams {
            rolling_restart: true,
            ..UpdateParams::default()
        };

        let err = build_update_plan(&containers, &params).expect_err("should reject");

        assert!(err.contains("rolling restarts"));
    }

    #[test]
    fn build_watchtower_instance_cleanup_plan_keeps_newest_instance_and_dedupes_images() {
        let containers = vec![
            MockContainer {
                created_at: "2024-06-18T11:58:00Z".to_string(),
                ..container("old-a", "/watchtower-old-a", &[], "sha256:wt-a")
            },
            MockContainer {
                created_at: "2024-06-18T11:59:00Z".to_string(),
                ..container("old-b", "/watchtower-old-b", &[], "sha256:wt-a")
            },
            MockContainer {
                created_at: "2024-06-18T12:00:00Z".to_string(),
                ..container("new", "/watchtower-new", &[], "sha256:wt-new")
            },
        ];

        let plan = build_watchtower_instance_cleanup_plan(&containers, true);

        assert_eq!(
            plan.stop_container_ids,
            vec![ContainerID::from("old-a"), ContainerID::from("old-b")]
        );
        assert_eq!(plan.cleanup_image_ids, vec![ImageID::from("sha256:wt-a")]);
    }

    #[test]
    fn build_watchtower_instance_cleanup_plan_skips_cleanup_when_disabled() {
        let containers = vec![
            MockContainer {
                created_at: "2024-06-18T11:58:00Z".to_string(),
                ..container("old", "/watchtower-old", &[], "sha256:wt-old")
            },
            MockContainer {
                created_at: "2024-06-18T12:00:00Z".to_string(),
                ..container("new", "/watchtower-new", &[], "sha256:wt-new")
            },
        ];

        let plan = build_watchtower_instance_cleanup_plan(&containers, false);

        assert_eq!(plan.stop_container_ids, vec![ContainerID::from("old")]);
        assert!(plan.cleanup_image_ids.is_empty());
    }

    #[test]
    fn build_watchtower_instance_cleanup_plan_sorts_by_created_timestamp() {
        let containers = vec![
            MockContainer {
                created_at: "2024-06-18T12:00:00Z".to_string(),
                ..container("new", "/watchtower-new", &[], "sha256:wt-new")
            },
            MockContainer {
                created_at: "2024-06-18T11:58:00Z".to_string(),
                ..container("old-a", "/watchtower-old-a", &[], "sha256:wt-a")
            },
            MockContainer {
                created_at: "2024-06-18T11:59:00Z".to_string(),
                ..container("old-b", "/watchtower-old-b", &[], "sha256:wt-a")
            },
        ];

        let plan = build_watchtower_instance_cleanup_plan(&containers, true);

        assert_eq!(
            plan.stop_container_ids,
            vec![ContainerID::from("old-a"), ContainerID::from("old-b")]
        );
        assert_eq!(plan.cleanup_image_ids, vec![ImageID::from("sha256:wt-a")]);
    }

    #[test]
    fn check_for_multiple_watchtower_instances_is_a_no_op_for_single_container() {
        let containers = vec![container("only", "/watchtower", &[], "sha256:only")];

        assert_eq!(check_for_multiple_watchtower_instances(&containers, true), None);
    }

    #[test]
    fn check_for_multiple_watchtower_instances_returns_cleanup_plan_for_many_containers() {
        let containers = vec![
            MockContainer {
                created_at: "2024-06-18T11:58:00Z".to_string(),
                ..container("old-a", "/watchtower-old-a", &[], "sha256:wt-a")
            },
            MockContainer {
                created_at: "2024-06-18T12:00:00Z".to_string(),
                ..container("new", "/watchtower-new", &[], "sha256:wt-new")
            },
        ];

        let plan = check_for_multiple_watchtower_instances(&containers, true)
            .expect("cleanup plan should exist");

        assert_eq!(plan.stop_container_ids, vec![ContainerID::from("old-a")]);
        assert_eq!(plan.cleanup_image_ids, vec![ImageID::from("sha256:wt-a")]);
    }

    #[test]
    fn update_scans_and_stops_stale_containers_without_restart() {
        let stale = update_container("stale", "stale");
        let fresh = update_container("fresh", "fresh");
        let client = MockUpdateClient::new(
            vec![stale.clone(), fresh.clone()],
            [
                Ok((true, ImageID::from("sha256:new"))),
                Ok((false, ImageID::from(""))),
            ],
        );
        let params = UpdateParams {
            no_restart: true,
            ..UpdateParams::default()
        };

        let report = update(&client, &params).expect("update should succeed");

        assert_eq!(client.stop_calls.borrow().len(), 1);
        assert_eq!(client.start_calls.borrow().len(), 0);
        assert_eq!(report.updated.len(), 1);
        assert_eq!(report.fresh.len(), 1);
        assert_eq!(report.updated[0].id, stale.id().clone());
        assert_eq!(report.fresh[0].id, fresh.id().clone());
    }
}
