#![forbid(unsafe_code)]

use crate::types::{ContainerID, ContainerReport, ImageID};

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

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

    pub(super) fn into_report(self) -> ContainerReport {
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
}
