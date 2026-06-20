#![forbid(unsafe_code)]

//! Session progress and report aggregation translated from the legacy
//! Watchtower update pipeline.
//!
//! The module stays Docker-agnostic. It only needs a small container view so
//! the next runtime slice can feed it data without dragging in API clients.

use std::collections::HashMap;

use crate::types::{ContainerID, ContainerReport, ImageID, Report};

/// State of a container during a Watchtower session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    /// Return the legacy display label for the state.
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

/// Minimal container view used by the session layer.
pub trait SessionContainer {
    fn id(&self) -> &ContainerID;
    fn name(&self) -> &str;
    fn image_name(&self) -> &str;
    fn safe_image_id(&self) -> &ImageID;
}

/// Container status recorded during a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerStatus {
    container_id: ContainerID,
    old_image: ImageID,
    new_image: ImageID,
    container_name: String,
    image_name: String,
    error: Option<String>,
    state: State,
}

impl ContainerStatus {
    /// Build a new container status from the runtime container view.
    pub fn from_container(container: &impl SessionContainer, new_image: ImageID, state: State) -> Self {
        Self {
            container_id: container.id().clone(),
            old_image: container.safe_image_id().clone(),
            new_image,
            container_name: container.name().to_owned(),
            image_name: container.image_name().to_owned(),
            error: None,
            state,
        }
    }

    /// Return the container identifier.
    pub fn id(&self) -> &ContainerID {
        &self.container_id
    }

    /// Return the container name.
    pub fn name(&self) -> &str {
        &self.container_name
    }

    /// Return the image used when the session started.
    pub fn current_image_id(&self) -> &ImageID {
        &self.old_image
    }

    /// Return the newest image found during the session.
    pub fn latest_image_id(&self) -> &ImageID {
        &self.new_image
    }

    /// Return the image name with tag used by the container.
    pub fn image_name(&self) -> &str {
        &self.image_name
    }

    /// Return the error captured for the container, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Return the state as a legacy label.
    pub fn state(&self) -> &'static str {
        self.state.as_str()
    }

    /// Update the error value.
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    /// Update the state value.
    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    /// Return true when the container has already been marked as stale.
    pub fn is_stale(&self) -> bool {
        self.state == State::Stale
    }

    fn to_report(&self) -> ContainerReport {
        ContainerReport {
            id: self.container_id.clone(),
            name: self.container_name.clone(),
            current_image_id: self.old_image.clone(),
            latest_image_id: self.new_image.clone(),
            image_name: self.image_name.clone(),
            error: self.error.clone(),
            state: self.state.as_str().to_owned(),
        }
    }
}

/// Current session progress, keyed by container identifier.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    entries: HashMap<ContainerID, ContainerStatus>,
}

impl Progress {
    /// Create an empty progress tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a recorded container status.
    pub fn add(&mut self, update: ContainerStatus) {
        self.entries.insert(update.container_id.clone(), update);
    }

    /// Add a container with the `Skipped` state.
    pub fn add_skipped(&mut self, container: &impl SessionContainer, error: impl Into<String>) {
        let mut update = ContainerStatus::from_container(
            container,
            container.safe_image_id().clone(),
            State::Skipped,
        );
        update.set_error(error);
        self.add(update);
    }

    /// Add a container with the `Scanned` state.
    pub fn add_scanned(&mut self, container: &impl SessionContainer, new_image: ImageID) {
        self.add(ContainerStatus::from_container(container, new_image, State::Scanned));
    }

    /// Mark the given container as having failed during the session.
    pub fn update_failed(&mut self, failures: impl IntoIterator<Item = (ContainerID, String)>) {
        for (id, error) in failures {
            if let Some(update) = self.entries.get_mut(&id) {
                update.set_error(error);
                update.set_state(State::Failed);
            }
        }
    }

    /// Mark the given container as queued for update.
    pub fn mark_for_update(&mut self, container_id: &ContainerID) {
        if let Some(update) = self.entries.get_mut(container_id) {
            update.set_state(State::Updated);
        }
    }

    /// Build the legacy report structure from the recorded progress.
    pub fn report(&self) -> Report {
        let mut report = Report::default();

        for status in self.entries.values() {
            if status.state == State::Skipped {
                report.skipped.push(status.to_report());
                continue;
            }

            report.scanned.push(status.to_report());

            if status.current_image_id() == status.latest_image_id() {
                let mut fresh = status.to_report();
                fresh.state = State::Fresh.as_str().to_owned();
                report.fresh.push(fresh);
                continue;
            }

            match status.state {
                State::Updated => report.updated.push(status.to_report()),
                State::Failed => report.failed.push(status.to_report()),
                _ => {
                    let mut stale = status.to_report();
                    stale.state = State::Stale.as_str().to_owned();
                    report.stale.push(stale);
                }
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

    /// Return true when no container status entries have been recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn sort_reports(reports: &mut [ContainerReport]) {
    reports.sort_by(|a, b| a.id.as_ref().cmp(b.id.as_ref()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct MockContainer {
        id: ContainerID,
        name: String,
        image_name: String,
        safe_image_id: ImageID,
    }

    impl SessionContainer for MockContainer {
        fn id(&self) -> &ContainerID {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn image_name(&self) -> &str {
            &self.image_name
        }

        fn safe_image_id(&self) -> &ImageID {
            &self.safe_image_id
        }
    }

    fn container(id: &str, name: &str, image: &str) -> MockContainer {
        MockContainer {
            id: ContainerID::from(id),
            name: name.to_owned(),
            image_name: image.to_owned(),
            safe_image_id: ImageID::from("sha256:old"),
        }
    }

    #[test]
    fn state_strings_match_legacy_labels() {
        assert_eq!(State::Skipped.as_str(), "Skipped");
        assert_eq!(State::Stale.as_str(), "Stale");
        assert_eq!(State::Unknown.as_str(), "Unknown");
    }

    #[test]
    fn progress_handles_skipped_scanned_and_failed_entries() {
        let first = container("sha256:bbbbbbbbbbbb", "/alpha", "repo:1");
        let second = container("sha256:aaaaaaaaaaaa", "/beta", "repo:2");

        let mut progress = Progress::new();
        progress.add_skipped(&first, "skip");
        progress.add_scanned(&second, ImageID::from("sha256:new"));
        progress.mark_for_update(second.id());
        progress.update_failed(vec![(second.id().clone(), "boom".to_owned())]);

        let report = progress.report();

        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.scanned.len(), 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.updated.len(), 0);
        assert_eq!(report.stale.len(), 0);
        assert_eq!(report.fresh.len(), 0);
        assert_eq!(report.all().count(), 3);
        assert!(report
            .failed
            .iter()
            .any(|entry| entry.error.as_deref() == Some("boom")));
    }

    #[test]
    fn progress_marks_fresh_and_stale_entries() {
        let fresh = MockContainer {
            safe_image_id: ImageID::from("sha256:same"),
            ..container("sha256:fresh", "/fresh", "repo:latest")
        };
        let stale = MockContainer {
            safe_image_id: ImageID::from("sha256:old"),
            ..container("sha256:stale", "/stale", "repo:latest")
        };

        let mut progress = Progress::new();
        progress.add_scanned(&fresh, ImageID::from("sha256:same"));
        progress.add_scanned(&stale, ImageID::from("sha256:new"));

        let report = progress.report();

        assert_eq!(report.fresh.len(), 1);
        assert_eq!(report.stale.len(), 1);
        assert_eq!(report.scanned.len(), 2);
        assert!(report.fresh[0].state == "Fresh");
        assert!(report.stale[0].state == "Stale");
    }
}
