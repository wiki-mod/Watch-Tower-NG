#![forbid(unsafe_code)]

use std::collections::HashMap;

use super::container_status::{ContainerLike, ContainerStatus, State};
use crate::types::{ContainerID, ImageID, Report};

/// Session progress indexed by container ID.
/// Mirrors Go's `type Progress map[types.ContainerID]*ContainerStatus`.
#[derive(Default)]
pub struct Progress(pub(super) HashMap<ContainerID, ContainerStatus>);

/// Build a ContainerStatus from a container-like value and state.
/// Mirrors Go's `UpdateFromContainer` in progress.go.
pub(super) fn update_from_container(
    cont: &impl ContainerLike,
    new_image: ImageID,
    state: State,
) -> ContainerStatus {
    ContainerStatus {
        container_id: cont.id().clone(),
        container_name: cont.name().to_string(),
        image_name: cont.image_name().to_string(),
        old_image: cont.safe_image_id(),
        new_image,
        error: None,
        state,
    }
}

impl Progress {
    /// Add a container to the map using container ID as the key.
    /// Mirrors Go's `Progress.Add`.
    pub fn add(&mut self, update: ContainerStatus) {
        self.0.insert(update.container_id.clone(), update);
    }

    /// Add a skipped container to the session.
    /// Mirrors Go's `Progress.AddSkipped`.
    pub fn add_skipped(&mut self, cont: &impl ContainerLike, err: impl ToString) {
        let safe_image = cont.safe_image_id();
        let mut update = update_from_container(cont, safe_image, State::Skipped);
        update.error = Some(err.to_string());
        self.add(update);
    }

    /// Add a scanned container to the session.
    /// Mirrors Go's `Progress.AddScanned`.
    pub fn add_scanned(&mut self, cont: &impl ContainerLike, new_image: impl Into<ImageID>) {
        self.add(update_from_container(cont, new_image.into(), State::Scanned));
    }

    /// Mark containers as failed.
    /// Mirrors Go's `Progress.UpdateFailed`.
    /// Panics if a container ID is not in the map, matching Go's direct indexing behavior.
    pub fn update_failed<E>(&mut self, failures: impl IntoIterator<Item = (ContainerID, E)>)
    where
        E: ToString,
    {
        for (id, err) in failures {
            let entry = self
                .0
                .get_mut(&id)
                .expect("container ID must exist in progress map");
            entry.error = Some(err.to_string());
            entry.state = State::Failed;
        }
    }

    /// Mark a previously scanned container as selected for update.
    /// Mirrors Go's `Progress.MarkForUpdate`.
    /// Panics if the container ID is not in the map, matching Go's direct indexing behavior.
    pub fn mark_for_update(&mut self, container_id: &ContainerID) {
        let entry = self
            .0
            .get_mut(container_id)
            .expect("container ID must exist in progress map");
        entry.state = State::Updated;
    }

    /// Create a Report from this Progress instance.
    /// Mirrors Go's `Progress.Report`.
    pub fn report(&self) -> Report {
        super::report::new_report(self)
    }
}

impl From<Progress> for Report {
    fn from(value: Progress) -> Self {
        super::report::new_report(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContainerID;

    #[derive(Clone)]
    struct MockContainer {
        id: ContainerID,
        name: String,
        image_name: String,
        current_image_id: ImageID,
    }

    impl ContainerLike for MockContainer {
        fn id(&self) -> &ContainerID {
            &self.id
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn image_name(&self) -> &str {
            &self.image_name
        }
        fn current_image_id(&self) -> &ImageID {
            &self.current_image_id
        }
        fn safe_image_id(&self) -> ImageID {
            self.current_image_id.clone()
        }
    }

    fn container(id: &str, name: &str, image_name: &str, current_image_id: &str) -> MockContainer {
        MockContainer {
            id: ContainerID::from(id),
            name: name.to_string(),
            image_name: image_name.to_string(),
            current_image_id: ImageID::from(current_image_id),
        }
    }

    #[test]
    fn skipped_and_scanned_entries_classify_correctly() {
        let scanned = container("b", "scanned", "image:scanned", "old");
        let skipped = container("a", "skipped", "image:skipped", "same");

        let mut progress = Progress::default();
        progress.add_scanned(&scanned, "next");
        progress.add_skipped(&skipped, "disabled by label");

        let report = progress.report();

        assert_eq!(report.scanned.len(), 1);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.scanned[0].id, ContainerID::from("b"));
        assert_eq!(report.skipped[0].id, ContainerID::from("a"));
        assert_eq!(report.skipped[0].state, "Skipped");
    }

    #[test]
    fn update_failure_marks_entry_failed() {
        let cont = container("c", "updating", "image:update", "old");
        let mut progress = Progress::default();

        progress.add_scanned(&cont, "new");
        progress.mark_for_update(cont.id());
        progress.update_failed([(cont.id().clone(), "pull failed")]);

        let report = progress.report();

        assert_eq!(report.scanned.len(), 1);
        assert_eq!(report.updated.len(), 0);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].state, "Failed");
        assert_eq!(report.failed[0].error.as_deref(), Some("pull failed"));
    }
}
