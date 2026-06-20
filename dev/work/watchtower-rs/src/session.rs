use std::collections::HashMap;
use std::fmt;

use crate::types::{ContainerID, ContainerReport, ImageID, Report};

/// Session state for a container during a single update run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Unknown,
    Skipped,
    Scanned,
    Updated,
    Failed,
    Fresh,
    Stale,
}

impl State {
    /// Return the legacy report label for this state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Skipped => "Skipped",
            Self::Scanned => "Scanned",
            Self::Updated => "Updated",
            Self::Failed => "Failed",
            Self::Fresh => "Fresh",
            Self::Stale => "Stale",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Minimal container view used to build session status without Docker types.
pub trait ContainerLike {
    fn id(&self) -> &ContainerID;
    fn name(&self) -> &str;
    fn image_name(&self) -> &str;
    fn current_image_id(&self) -> &ImageID;
}

/// One container entry in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerStatus {
    pub container_id: ContainerID,
    pub current_image_id: ImageID,
    pub latest_image_id: ImageID,
    pub container_name: String,
    pub image_name: String,
    pub error: Option<String>,
    pub state: State,
}

impl ContainerStatus {
    /// Build a status record from any container-like value.
    pub fn from_container(
        container: &impl ContainerLike,
        latest_image_id: impl Into<ImageID>,
        state: State,
    ) -> Self {
        Self {
            container_id: container.id().clone(),
            current_image_id: container.current_image_id().clone(),
            latest_image_id: latest_image_id.into(),
            container_name: container.name().to_string(),
            image_name: container.image_name().to_string(),
            error: None,
            state,
        }
    }

    /// Return the container ID.
    pub fn id(&self) -> &ContainerID {
        &self.container_id
    }

    /// Return the container name.
    pub fn name(&self) -> &str {
        &self.container_name
    }

    /// Return the image ID that the container used when the session started.
    pub fn current_image_id(&self) -> &ImageID {
        &self.current_image_id
    }

    /// Return the newest image ID found during the session.
    pub fn latest_image_id(&self) -> &ImageID {
        &self.latest_image_id
    }

    /// Return the name:tag that the container uses.
    pub fn image_name(&self) -> &str {
        &self.image_name
    }

    /// Return the error, if any, that was encountered for the container during a session.
    pub fn error(&self) -> &str {
        self.error.as_deref().unwrap_or("")
    }

    /// Return the current state label.
    pub fn state(&self) -> &'static str {
        self.state.as_str()
    }

    /// Return the state label used in the aggregated report.
    pub fn state_label(&self) -> &'static str {
        self.state.as_str()
    }

    fn into_report(self) -> ContainerReport {
        let state = self.state_label().to_string();
        ContainerReport {
            id: self.container_id,
            name: self.container_name,
            current_image_id: self.current_image_id,
            latest_image_id: self.latest_image_id,
            image_name: self.image_name,
            error: self.error,
            state,
        }
    }
}

/// Session progress indexed by container ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    entries: HashMap<ContainerID, ContainerStatus>,
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

    /// Mark a previously scanned container as skipped.
    pub fn mark_skipped(&mut self, container_id: &ContainerID, err: impl ToString) {
        if let Some(entry) = self.entries.get_mut(container_id) {
            entry.state = State::Skipped;
            entry.error = Some(err.to_string());
        }
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
        let mut report = Report::default();

        for status in self.entries.values().cloned() {
            let state = status.state;

            if state == State::Skipped {
                report.skipped.push(status.into_report());
                continue;
            }

            let mut report_entry = status.into_report();
            report.scanned.push(report_entry.clone());

            if report_entry.current_image_id == report_entry.latest_image_id {
                report_entry.state = State::Fresh.as_str().to_string();
                report.fresh.push(report_entry);
                continue;
            }

            match state {
                State::Updated => report.updated.push(report_entry),
                State::Failed => report.failed.push(report_entry),
                State::Unknown | State::Scanned | State::Fresh | State::Stale => {
                    report_entry.state = State::Stale.as_str().to_string();
                    report.stale.push(report_entry);
                }
                State::Skipped => unreachable!("skipped entries are handled earlier"),
            }
        }

        sort_reports(&mut report.scanned);
        sort_reports(&mut report.updated);
        sort_reports(&mut report.failed);
        sort_reports(&mut report.skipped);
        sort_reports(&mut report.stale);
        sort_reports(&mut report.fresh);

        report
    }
}

impl From<Progress> for Report {
    fn from(value: Progress) -> Self {
        value.report()
    }
}

fn sort_reports(reports: &mut [ContainerReport]) {
    reports.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn status_mapping_preserves_container_fields_and_labels() {
        let container = container("sha256:1234567890abcdef", "app", "example/app:latest", "old");

        let status = ContainerStatus::from_container(&container, "new", State::Updated);

        assert_eq!(status.id(), &ContainerID::from("sha256:1234567890abcdef"));
        assert_eq!(status.name(), "app");
        assert_eq!(status.image_name(), "example/app:latest");
        assert_eq!(status.current_image_id(), &ImageID::from("old"));
        assert_eq!(status.latest_image_id(), &ImageID::from("new"));
        assert_eq!(status.error(), "");
        assert_eq!(status.state(), "Updated");
        assert_eq!(status.state_label(), "Updated");
        assert_eq!(State::Unknown.as_str(), "Unknown");
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

    #[test]
    fn report_conversion_sorts_and_classifies_buckets() {
        let fresh = container("d", "fresh", "image:fresh", "same");
        let updated = container("c", "updated", "image:updated", "old");
        let failed = container("b", "failed", "image:failed", "old");
        let skipped = container("e", "skipped", "image:skipped", "old");
        let stale = container("a", "stale", "image:stale", "old");

        let mut progress = Progress::default();
        progress.add_scanned(&stale, "new-stale");
        progress.add_scanned(&failed, "new-failed");
        progress.add_scanned(&updated, "new-updated");
        progress.add_scanned(&fresh, "same");
        progress.add_scanned(&skipped, "new-skipped");

        progress.mark_for_update(updated.id());
        progress.mark_for_update(failed.id());
        progress.update_failed([(failed.id().clone(), "network error")]);
        progress.mark_skipped(skipped.id(), "container disappeared");

        let report = progress.report();

        assert_eq!(
            report
                .scanned
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "d"]
        );
        assert_eq!(
            report
                .updated
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
        assert_eq!(
            report
                .failed
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
        assert_eq!(
            report
                .skipped
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["e"]
        );
        assert_eq!(
            report
                .fresh
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["d"]
        );
        assert_eq!(
            report
                .stale
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(report.stale[0].state, "Stale");
    }
}
