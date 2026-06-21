#![forbid(unsafe_code)]

use crate::sorter::sort_by_created_at;
use crate::types::{ContainerID, ImageID, RuntimeContainer};
use std::collections::HashSet;

/// Cleanup work for older watchtower instances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchtowerInstanceCleanupPlan {
    pub stop_container_ids: Vec<ContainerID>,
    pub cleanup_image_ids: Vec<ImageID>,
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

/// Return the cleanup work required when more than one watchtower instance is present.
pub fn check_for_multiple_watchtower_instances<C: RuntimeContainer + Clone>(
    containers: &[C],
    cleanup: bool,
) -> Option<WatchtowerInstanceCleanupPlan> {
    if containers.len() <= 1 {
        return None;
    }

    Some(build_watchtower_instance_cleanup_plan(containers, cleanup))
}

/// Build the cleanup plan for older watchtower instances.
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

fn dedupe_cleanup_image_ids(image_ids: impl IntoIterator<Item = ImageID>) -> Vec<ImageID> {
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
