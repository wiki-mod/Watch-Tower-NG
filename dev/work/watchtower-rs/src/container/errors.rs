#![forbid(unsafe_code)]

use std::fmt;

/// Opaque container error, equivalent to Go's `errors.New()` variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContainerError(pub &'static str);

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ContainerError {}

pub const ERROR_NO_IMAGE_INFO: ContainerError =
    ContainerError("no available image info");
pub const ERROR_NO_CONTAINER_INFO: ContainerError =
    ContainerError("no available container info");
pub const ERROR_INVALID_CONFIG: ContainerError =
    ContainerError("container configuration missing or invalid");
pub const ERROR_LABEL_NOT_FOUND: ContainerError =
    ContainerError("label was not found in container");
