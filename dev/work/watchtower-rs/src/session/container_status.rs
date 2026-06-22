#![forbid(unsafe_code)]

use crate::types::{ContainerID, ContainerReport, ImageID};

/// Trait matching the Go types.Container interface used in progress.go.
/// Necessary in Rust because modules cannot access sibling-package private
/// fields the way Go files in the same package can.
pub trait ContainerLike {
    fn id(&self) -> &ContainerID;
    fn name(&self) -> &str;
    fn image_name(&self) -> &str;
    fn current_image_id(&self) -> &ImageID;
}

/// Session state for a container during a single update run.
/// Mirrors the Go `State` iota in container_status.go.
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

/// One container entry in a session. Mirrors Go's ContainerStatus struct.
/// Fields are pub(super) so progress.rs can construct and mutate within the
/// session module, mirroring Go's same-package field access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerStatus {
    pub(super) container_id: ContainerID,
    pub(super) old_image: ImageID,
    pub(super) new_image: ImageID,
    pub(super) container_name: String,
    pub(super) image_name: String,
    pub(super) error: Option<String>,
    pub(super) state: State,
}

impl ContainerStatus {
    /// Return the container ID.
    pub fn id(&self) -> ContainerID {
        self.container_id.clone()
    }

    /// Return the container name.
    pub fn name(&self) -> &str {
        &self.container_name
    }

    /// Return the image ID the container used when the session started.
    pub fn current_image_id(&self) -> ImageID {
        self.old_image.clone()
    }

    /// Return the newest image ID found during the session.
    pub fn latest_image_id(&self) -> ImageID {
        self.new_image.clone()
    }

    /// Return the name:tag the container uses.
    pub fn image_name(&self) -> &str {
        &self.image_name
    }

    /// Return the error encountered for this container, or empty string.
    pub fn error(&self) -> String {
        self.error.clone().unwrap_or_default()
    }

    /// Return the current state label.
    pub fn state(&self) -> &'static str {
        self.state.as_str()
    }

    /// Convert into a ContainerReport for the aggregated report.
    /// pub(super) because report.rs accesses this within the session module,
    /// mirroring Go's same-package field access in report.go.
    pub(super) fn into_report(self) -> ContainerReport {
        ContainerReport {
            id: self.container_id,
            name: self.container_name,
            current_image_id: self.old_image,
            latest_image_id: self.new_image,
            image_name: self.image_name,
            error: self.error,
            state: self.state.as_str().to_string(),
        }
    }
}
