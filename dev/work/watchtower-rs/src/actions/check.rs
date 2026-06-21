#![forbid(unsafe_code)]

use crate::sorter::sort_by_created_at;
use crate::types::{ContainerID, ImageID, RuntimeContainer};
use tracing::{debug, info};

/// Cleanup work for older watchtower instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchtowerInstanceCleanupPlan {
    pub stop_container_ids: Vec<ContainerID>,
    pub cleanup_image_ids: Vec<ImageID>,
}

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
/// will stop and remove all but the most recently started container. This behaviour can be bypassed
/// if a scope UID is defined.
///
/// Returns a cleanup plan with the containers to stop and optionally the images to remove,
/// or None if there are 1 or fewer instances.
///
/// Mirrors `CheckForMultipleWatchtowerInstances(client container.Client, cleanup bool, scope string) error`
/// from `internal/actions/check.go`. The client/scope parameters are handled at the call site; this
/// function only validates and orders the containers for cleanup.
pub fn check_for_multiple_watchtower_instances<C: RuntimeContainer + Clone>(
    containers: &[C],
    cleanup: bool,
) -> Option<WatchtowerInstanceCleanupPlan> {
    if containers.len() <= 1 {
        debug!("There are no additional watchtower containers");
        return None;
    }

    info!("Found multiple running watchtower instances. Cleaning up.");
    cleanup_excess_watchtowers(containers, cleanup)
}

fn cleanup_excess_watchtowers<C: RuntimeContainer + Clone>(
    containers: &[C],
    cleanup: bool,
) -> Option<WatchtowerInstanceCleanupPlan> {
    let mut ordered = sort_by_created_at(containers);
    // Remove the most recently created (last in sorted order), keep all others for stopping
    if !ordered.is_empty() {
        ordered.pop();
    }

    let stop_container_ids = ordered.iter().map(|c| c.id().clone()).collect();

    let cleanup_image_ids = if cleanup {
        ordered
            .iter()
            .map(|c| c.image_id().clone())
            .filter(|id| !id.as_str().is_empty())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    Some(WatchtowerInstanceCleanupPlan {
        stop_container_ids,
        cleanup_image_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContainerID, ImageID, UpdateParams};

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

    fn test_container(id: &str, name: &str, image_id: &str, created_at: &str) -> TestContainer {
        TestContainer {
            id: ContainerID::from(id),
            name: name.to_string(),
            links: Vec::new(),
            image_id: ImageID::from(image_id),
            created_at: created_at.to_string(),
        }
    }

    #[test]
    fn check_for_sanity_allows_no_links() {
        let c = test_container("1", "test", "sha256:img", "2024-06-18T12:00:00Z");
        assert!(check_for_sanity(&[c], true).is_ok());
    }

    #[test]
    fn check_for_sanity_rejects_links_with_rolling_restarts() {
        let mut c = test_container("1", "test", "sha256:img", "2024-06-18T12:00:00Z");
        c.links = vec!["/db".to_string()];
        let result = check_for_sanity(&[c], true);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not compatible with rolling restarts"));
    }

    #[test]
    fn check_for_sanity_allows_links_without_rolling_restarts() {
        let mut c = test_container("1", "test", "sha256:img", "2024-06-18T12:00:00Z");
        c.links = vec!["/db".to_string()];
        assert!(check_for_sanity(&[c], false).is_ok());
    }

    #[test]
    fn check_for_multiple_watchtower_instances_returns_none_for_single() {
        let c = test_container("1", "wt1", "sha256:wt", "2024-06-18T12:00:00Z");
        assert!(check_for_multiple_watchtower_instances(&[c], false).is_none());
    }

    #[test]
    fn check_for_multiple_watchtower_instances_returns_plan_for_multiple() {
        let c1 = test_container("1", "wt1", "sha256:wt1", "2024-06-18T11:00:00Z");
        let c2 = test_container("2", "wt2", "sha256:wt2", "2024-06-18T12:00:00Z");

        let plan = check_for_multiple_watchtower_instances(&[c1.clone(), c2.clone()], false)
            .expect("plan should exist");

        // Should return the older one (c1) for stopping
        assert_eq!(plan.stop_container_ids.len(), 1);
        assert_eq!(plan.stop_container_ids[0], c1.id);
        // No cleanup without cleanup flag
        assert!(plan.cleanup_image_ids.is_empty());
    }

    #[test]
    fn check_for_multiple_watchtower_instances_keeps_newest() {
        let c1 = test_container("1", "wt1", "sha256:wt1", "2024-06-18T11:00:00Z");
        let c2 = test_container("2", "wt2", "sha256:wt2", "2024-06-18T12:00:00Z");
        let c3 = test_container("3", "wt3", "sha256:wt3", "2024-06-18T11:30:00Z");

        let plan = check_for_multiple_watchtower_instances(&[c1.clone(), c2.clone(), c3.clone()], true)
            .expect("plan should exist");

        // Should return all except the newest (c2)
        assert_eq!(plan.stop_container_ids.len(), 2);
        assert!(plan.stop_container_ids.contains(&c1.id));
        assert!(plan.stop_container_ids.contains(&c3.id));
        assert!(!plan.stop_container_ids.contains(&c2.id));

        // With cleanup, should include image IDs (excluding c2's image)
        assert_eq!(plan.cleanup_image_ids.len(), 2);
        assert!(plan.cleanup_image_ids.contains(&c1.image_id));
        assert!(plan.cleanup_image_ids.contains(&c3.image_id));
    }
}
