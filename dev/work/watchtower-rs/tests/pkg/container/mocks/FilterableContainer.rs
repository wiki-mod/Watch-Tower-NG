#![forbid(unsafe_code)]

/// Test double for the legacy `FilterableContainer` mock.
///
/// The Go version delegated every method to `testify/mock`. This Rust version
/// keeps the same surface by storing the values each method should return.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilterableContainer {
    pub enabled: (bool, bool),
    pub is_watchtower: bool,
    pub name: String,
    pub scope: (String, bool),
    pub image_name: String,
}

impl FilterableContainer {
    /// Return the configured enabled state and presence flag.
    pub fn enabled(&self) -> (bool, bool) {
        self.enabled
    }

    /// Return whether the mock represents the Watchtower container.
    pub fn is_watchtower(&self) -> bool {
        self.is_watchtower
    }

    /// Return the configured container name.
    pub fn name(&self) -> String {
        self.name.clone()
    }

    /// Return the configured scope and presence flag.
    pub fn scope(&self) -> (String, bool) {
        self.scope.clone()
    }

    /// Return the configured image name.
    pub fn image_name(&self) -> String {
        self.image_name.clone()
    }
}
