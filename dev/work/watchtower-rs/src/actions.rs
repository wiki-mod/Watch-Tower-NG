#![forbid(unsafe_code)]

//! Pure runtime action helpers translated from the legacy Go implementation.
//!
//! This module intentionally stays Docker-agnostic. It only encodes the
//! container-ordering and restart decisions that can be derived from in-memory
//! state.

use crate::types::{ContainerID, RuntimeContainer, UpdateParams};
use crate::types::ImageID;
use std::collections::HashSet;

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
        .map(|container| container.image_id().as_str().to_string())
        .collect();

    dedupe_cleanup_image_ids(image_ids)
        .into_iter()
        .filter(|image_id| !in_use.contains(image_id.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct MockContainer {
        id: ContainerID,
        name: String,
        links: Vec<String>,
        image_id: ImageID,
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

    fn container(id: &str, name: &str, links: &[&str], image_id: &str) -> MockContainer {
        MockContainer {
            id: ContainerID::from(id),
            name: name.to_string(),
            links: links.iter().map(|link| (*link).to_string()).collect(),
            image_id: ImageID::from(image_id),
            stale: false,
            linked_to_restarting: false,
            monitor_only: false,
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
}
