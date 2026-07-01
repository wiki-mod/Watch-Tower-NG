#![forbid(unsafe_code)]

use crate::container::Container;
use crate::sorter::sort_by_created_at;
use crate::types::RuntimeContainer;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// CheckForSanity makes sure everything is sane before starting.
///
/// If rolling_restarts is enabled, verifies that no containers have links to other containers,
/// as this is incompatible with rolling restart logic.
///
/// Mirrors `CheckForSanity(client container.Client, filter types.Filter, rollingRestarts bool) error`
/// from `internal/actions/check.go`. The client/filter parameters are handled at the call site.
pub fn check_for_sanity<C: RuntimeContainer>(
    containers: &[C],
    rolling_restarts: bool,
) -> Result<(), String> {
    debug!("Making sure everything is sane before starting");

    if rolling_restarts {
        for c in containers {
            if !c.links().is_empty() {
                return Err(format!(
                    "{:?} is depending on at least one other container. This is not compatible with rolling restarts",
                    c.name()
                ));
            }
        }
    }
    Ok(())
}

/// CheckForMultipleWatchtowerInstances ensures that there are not multiple instances of the
/// watchtower running simultaneously. If multiple watchtower containers are detected, this function
/// will stop and remove all but the most recently started container.
///
/// Executes container stops and image removals directly (mirroring Go semantics).
/// Returns Ok(()) if successful, or Err with a count of errors if stops failed.
///
/// Mirrors `CheckForMultipleWatchtowerInstances(client container.Client, cleanup bool, scope string) error`
/// from `internal/actions/check.go`. The scope filtering is applied at the call site.
pub fn check_for_multiple_watchtower_instances<C>(
    client: &C,
    containers: &[Container],
    cleanup: bool,
) -> Result<(), String>
where
    C: crate::actions::UpdateClient,
    C::Error: std::fmt::Display,
{
    if containers.len() <= 1 {
        debug!("There are no additional watchtower containers");
        return Ok(());
    }

    info!("Found multiple running watchtower instances. Cleaning up.");
    cleanup_excess_watchtowers(client, containers, cleanup)
}

fn cleanup_excess_watchtowers<C>(
    client: &C,
    containers: &[Container],
    cleanup: bool,
) -> Result<(), String>
where
    C: crate::actions::UpdateClient,
    C::Error: std::fmt::Display,
{
    let mut stop_errors = 0;
    let mut ordered = sort_by_created_at(containers);

    // Remove the most recently created (last in sorted order), keep all others for stopping
    if !ordered.is_empty() {
        ordered.pop();
    }

    for container in &ordered {
        match client.stop_container(container, Duration::from_secs(600)) {
            Ok(()) => {
                if cleanup {
                    if let Err(err) = client.remove_image_by_id(container.image_id()) {
                        warn!(
                            "Could not cleanup watchtower images, possibly because of other watchtowers instances in other scopes: {}",
                            err
                        );
                    }
                }
            }
            Err(err) => {
                error!("Could not stop a previous watchtower instance: {}", err);
                stop_errors += 1;
            }
        }
    }

    if stop_errors > 0 {
        Err(format!(
            "{} errors while stopping watchtower containers",
            stop_errors
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{ContainerConfig, ContainerInspect, ContainerState, HostConfig, ImageInspect};
    use crate::types::{ContainerID, ImageID, UpdateParams};
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct TestContainer {
        id: ContainerID,
        name: String,
        links: Vec<String>,
        image_id: ImageID,
        created_at: String,
    }

    impl RuntimeContainer for TestContainer {
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
            false
        }

        fn set_stale(&mut self, _value: bool) {}

        fn is_linked_to_restarting(&self) -> bool {
            false
        }

        fn set_linked_to_restarting(&mut self, _value: bool) {}

        fn is_monitor_only(&self, _params: &UpdateParams) -> bool {
            false
        }
    }

    fn test_container(id: &str, name: &str, image_id: &str, created_at_str: &str) -> Container {
        Container::new(
            ContainerInspect {
                id: ContainerID::new(id),
                name: format!("/{}", name),
                image: ImageID::new(image_id),
                created: created_at_str.to_string(),
                state: ContainerState {
                    running: true,
                    restarting: false,
                },
                config: Some(ContainerConfig {
                    image: "test:latest".to_string(),
                    labels: BTreeMap::new(),
                    working_dir: String::new(),
                    user: String::new(),
                    entrypoint: Vec::new(),
                    cmd: Vec::new(),
                    env: Vec::new(),
                    volumes: Default::default(),
                    exposed_ports: None,
                    healthcheck: None,
                    hostname: String::new(),
                }),
                host_config: Some(HostConfig {
                    links: Vec::new(),
                    network_mode: Default::default(),
                    port_bindings: Default::default(),
                    auto_remove: false,
                }),
                network_settings: None,
            },
            Some(ImageInspect {
                id: ImageID::new(image_id),
                config: ContainerConfig {
                    image: "test:latest".to_string(),
                    labels: BTreeMap::new(),
                    working_dir: String::new(),
                    user: String::new(),
                    entrypoint: Vec::new(),
                    cmd: Vec::new(),
                    env: Vec::new(),
                    volumes: Default::default(),
                    exposed_ports: None,
                    healthcheck: None,
                    hostname: String::new(),
                },
            }),
        )
    }

    /// Mock client that tracks stop and remove calls
    #[derive(Clone)]
    struct MockClient {
        stopped_ids: Arc<Mutex<Vec<ContainerID>>>,
        removed_image_ids: Arc<Mutex<Vec<ImageID>>>,
    }

    impl MockClient {
        fn new() -> Self {
            Self {
                stopped_ids: Arc::new(Mutex::new(Vec::new())),
                removed_image_ids: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn stopped_count(&self) -> usize {
            self.stopped_ids.lock().unwrap().len()
        }

        fn removed_image_count(&self) -> usize {
            self.removed_image_ids.lock().unwrap().len()
        }

        fn stopped_ids(&self) -> Vec<ContainerID> {
            self.stopped_ids.lock().unwrap().clone()
        }

        fn removed_image_ids(&self) -> Vec<ImageID> {
            self.removed_image_ids.lock().unwrap().clone()
        }
    }

    impl crate::lifecycle::LifecycleClient for MockClient {
        type Error = String;

        fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error> {
            Ok(Vec::new())
        }

        fn get_container(
            &self,
            _container_id: &ContainerID,
        ) -> std::result::Result<Container, Self::Error> {
            Err("not implemented".to_string())
        }

        fn execute_command(
            &self,
            _container_id: &ContainerID,
            _command: &str,
            _timeout_minutes: i64,
        ) -> std::result::Result<bool, Self::Error> {
            Err("not implemented".to_string())
        }
    }

    impl crate::actions::UpdateClient for MockClient {
        fn is_container_stale(
            &self,
            _container: &Container,
            _params: &UpdateParams,
        ) -> std::result::Result<(bool, ImageID), Self::Error> {
            Err("not implemented".to_string())
        }

        fn stop_container(
            &self,
            container: &Container,
            _timeout: Duration,
        ) -> std::result::Result<(), Self::Error> {
            self.stopped_ids.lock().unwrap().push(container.id().clone());
            Ok(())
        }

        fn start_container(
            &self,
            _container: &Container,
        ) -> std::result::Result<ContainerID, Self::Error> {
            Err("not implemented".to_string())
        }

        fn rename_container(
            &self,
            _container: &Container,
            _new_name: &str,
        ) -> std::result::Result<(), Self::Error> {
            Err("not implemented".to_string())
        }

        fn remove_image_by_id(&self, image_id: &ImageID) -> std::result::Result<(), Self::Error> {
            self.removed_image_ids.lock().unwrap().push(image_id.clone());
            Ok(())
        }
    }

    #[test]
    fn check_for_sanity_allows_no_links() {
        let c = test_container("1", "test", "sha256:img", "2024-06-18T12:00:00Z");
        let c_test = TestContainer {
            id: c.id().clone(),
            name: c.name().to_string(),
            links: Vec::new(),
            image_id: c.image_id().clone(),
            created_at: c.created_at().to_string(),
        };
        assert!(check_for_sanity(&[c_test], true).is_ok());
    }

    #[test]
    fn check_for_sanity_rejects_links_with_rolling_restarts() {
        let c = test_container("1", "test", "sha256:img", "2024-06-18T12:00:00Z");
        let c_test = TestContainer {
            id: c.id().clone(),
            name: c.name().to_string(),
            links: vec!["/db".to_string()],
            image_id: c.image_id().clone(),
            created_at: c.created_at().to_string(),
        };
        let result = check_for_sanity(&[c_test], true);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not compatible with rolling restarts"));
    }

    #[test]
    fn check_for_sanity_allows_links_without_rolling_restarts() {
        let c = test_container("1", "test", "sha256:img", "2024-06-18T12:00:00Z");
        let c_test = TestContainer {
            id: c.id().clone(),
            name: c.name().to_string(),
            links: vec!["/db".to_string()],
            image_id: c.image_id().clone(),
            created_at: c.created_at().to_string(),
        };
        assert!(check_for_sanity(&[c_test], false).is_ok());
    }

    #[test]
    fn check_for_multiple_watchtower_instances_returns_ok_for_single() {
        let client = MockClient::new();
        let c = test_container("1", "wt1", "sha256:wt", "2024-06-18T12:00:00Z");
        assert!(check_for_multiple_watchtower_instances(&client, &[c], false).is_ok());
        assert_eq!(client.stopped_count(), 0);
    }

    #[test]
    fn check_for_multiple_watchtower_instances_stops_older_without_cleanup() {
        let client = MockClient::new();
        let c1 = test_container("1", "wt1", "sha256:wt1", "2024-06-18T11:00:00Z");
        let c2 = test_container("2", "wt2", "sha256:wt2", "2024-06-18T12:00:00Z");

        assert!(check_for_multiple_watchtower_instances(&client, &[c1.clone(), c2.clone()], false)
            .is_ok());

        // Should have stopped the older one (c1)
        assert_eq!(client.stopped_count(), 1);
        assert_eq!(client.stopped_ids()[0], *c1.id());
        // No cleanup without cleanup flag
        assert_eq!(client.removed_image_count(), 0);
    }

    #[test]
    fn check_for_multiple_watchtower_instances_stops_and_removes_with_cleanup() {
        let client = MockClient::new();
        let c1 = test_container("1", "wt1", "sha256:wt1", "2024-06-18T11:00:00Z");
        let c2 = test_container("2", "wt2", "sha256:wt2", "2024-06-18T12:00:00Z");
        let c3 = test_container("3", "wt3", "sha256:wt3", "2024-06-18T11:30:00Z");

        assert!(check_for_multiple_watchtower_instances(&client, &[c1.clone(), c2.clone(), c3.clone()], true)
            .is_ok());

        // Should have stopped all except the newest (c2)
        assert_eq!(client.stopped_count(), 2);
        let stopped = client.stopped_ids();
        assert!(stopped.contains(&c1.id().clone()));
        assert!(stopped.contains(&c3.id().clone()));
        assert!(!stopped.contains(&c2.id().clone()));

        // With cleanup, should have removed images (excluding c2's image)
        assert_eq!(client.removed_image_count(), 2);
        let removed = client.removed_image_ids();
        assert!(removed.contains(&c1.image_id().clone()));
        assert!(removed.contains(&c3.image_id().clone()));
    }
}
