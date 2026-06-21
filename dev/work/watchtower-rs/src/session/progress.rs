#![forbid(unsafe_code)]

use std::collections::HashMap;

use super::container_status::{ContainerLike, ContainerStatus, State};
use crate::types::{ContainerID, ImageID, Report};

/// Session progress indexed by container ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    pub(super) entries: HashMap<ContainerID, ContainerStatus>,
}

impl Progress {
    /// Add or replace an entry.
    pub fn add(&mut self, update: ContainerStatus) {
        self.entries.insert(update.container_id.clone(), update);
    }

    /// Add a scanned container to the session.
    pub fn add_scanned(
        &mut self,
        container: &impl ContainerLike,
        latest_image_id: impl Into<ImageID>,
    ) {
        self.add(ContainerStatus::from_container(
            container,
            latest_image_id,
            State::Scanned,
        ));
    }

    /// Add a skipped container to the session.
    pub fn add_skipped(&mut self, container: &impl ContainerLike, err: impl ToString) {
        let mut update = ContainerStatus::from_container(
            container,
            container.current_image_id().clone(),
            State::Skipped,
        );
        update.error = Some(err.to_string());
        self.add(update);
    }

    /// Mark a previously scanned container as selected for update.
    pub fn mark_for_update(&mut self, container_id: &ContainerID) {
        self.entries
            .get_mut(container_id)
            .expect("container must exist before mark_for_update")
            .state = State::Updated;
    }


    /// Mark containers as failed.
    pub fn update_failed<E>(&mut self, failures: impl IntoIterator<Item = (ContainerID, E)>)
    where
        E: ToString,
    {
        for (container_id, err) in failures {
            let entry = self
                .entries
                .get_mut(&container_id)
                .expect("container must exist before update_failed");
            entry.state = State::Failed;
            entry.error = Some(err.to_string());
        }
    }

    /// Convert the session progress into the crate's aggregated report shape.
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
    use super::super::container_status::ContainerLike;
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
    fn skipped_and_scanned_entries_classify_like_the_legacy_session() {
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
        let container = container("c", "updating", "image:update", "old");
        let mut progress = Progress::default();

        progress.add_scanned(&container, "new");
        progress.mark_for_update(container.id());
        progress.update_failed([(container.id().clone(), "pull failed")]);

        let report = progress.report();

        assert_eq!(report.scanned.len(), 1);
        assert_eq!(report.updated.len(), 0);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].state, "Failed");
        assert_eq!(report.failed[0].error.as_deref(), Some("pull failed"));
    }
}
