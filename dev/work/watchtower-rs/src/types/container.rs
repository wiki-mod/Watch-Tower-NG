#![forbid(unsafe_code)]

//! Container identifier types and the Container trait.
//!
//! Translated from `old-source/pkg/types/container.go`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::container::{
    Container as ContainerModel, ContainerConfig, ContainerInspect, HostConfig, ImageInspect,
};
use crate::Result;

use super::update_params::UpdateParams;

/// Docker container identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContainerID(String);

impl ContainerID {
    /// Create a new container identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Return the raw identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the short 12-character identifier, without a `sha256:` prefix.
    pub fn short_id(&self) -> String {
        short_id(&self.0)
    }
}

impl From<&str> for ContainerID {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ContainerID {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ContainerID {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ContainerID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Docker image identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImageID(String);

impl ImageID {
    /// Create a new image identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Return the raw identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the short 12-character identifier, without a `sha256:` prefix.
    pub fn short_id(&self) -> String {
        short_id(&self.0)
    }
}

impl From<&str> for ImageID {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ImageID {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for ImageID {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ImageID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Legacy container interface translated from `pkg/types/container.go`.
pub trait Container {
    fn container_info(&self) -> Option<&ContainerInspect>;
    fn id(&self) -> &ContainerID;
    fn is_running(&self) -> bool;
    fn name(&self) -> &str;
    fn image_id(&self) -> &ImageID;
    fn safe_image_id(&self) -> ImageID;
    fn image_name(&self) -> &str;
    fn enabled(&self) -> (bool, bool);
    fn is_monitor_only(&self, params: &UpdateParams) -> bool;
    fn scope(&self) -> (Option<&str>, bool);
    fn links(&self) -> &[String];
    fn to_restart(&self) -> bool;
    fn is_watchtower(&self) -> bool;
    fn stop_signal(&self) -> String;
    fn has_image_info(&self) -> bool;
    fn image_info(&self) -> Option<&ImageInspect>;
    fn get_lifecycle_pre_check_command(&self) -> String;
    fn get_lifecycle_post_check_command(&self) -> String;
    fn get_lifecycle_pre_update_command(&self) -> String;
    fn get_lifecycle_post_update_command(&self) -> String;
    fn verify_configuration(&mut self) -> Result<()>;
    fn set_stale(&mut self, value: bool);
    fn is_stale(&self) -> bool;
    fn is_no_pull(&self, params: &UpdateParams) -> bool;
    fn set_linked_to_restarting(&mut self, value: bool);
    fn is_linked_to_restarting(&self) -> bool;
    fn pre_update_timeout(&self) -> i64;
    fn post_update_timeout(&self) -> i64;
    fn is_restarting(&self) -> bool;
    fn get_create_config(&mut self) -> Result<ContainerConfig>;
    fn get_create_host_config(&mut self) -> Result<HostConfig>;
}

impl Container for ContainerModel {
    fn container_info(&self) -> Option<&ContainerInspect> {
        ContainerModel::container_info(self)
    }

    fn id(&self) -> &ContainerID {
        ContainerModel::id(self)
    }

    fn is_running(&self) -> bool {
        ContainerModel::is_running(self)
    }

    fn name(&self) -> &str {
        ContainerModel::name(self)
    }

    fn image_id(&self) -> &ImageID {
        ContainerModel::image_id(self)
    }

    fn safe_image_id(&self) -> ImageID {
        ContainerModel::safe_image_id(self)
            .cloned()
            .unwrap_or_else(|| ImageID::new(""))
    }

    fn image_name(&self) -> &str {
        ContainerModel::image_name(self)
    }

    fn enabled(&self) -> (bool, bool) {
        ContainerModel::enabled(self)
    }

    fn is_monitor_only(&self, params: &UpdateParams) -> bool {
        ContainerModel::is_monitor_only(self, params)
    }

    fn scope(&self) -> (Option<&str>, bool) {
        let scope = ContainerModel::scope(self);
        (scope, scope.is_some())
    }

    fn links(&self) -> &[String] {
        ContainerModel::links(self)
    }

    fn to_restart(&self) -> bool {
        ContainerModel::to_restart(self)
    }

    fn is_watchtower(&self) -> bool {
        ContainerModel::is_watchtower(self)
    }

    fn stop_signal(&self) -> String {
        ContainerModel::stop_signal(self)
    }

    fn has_image_info(&self) -> bool {
        ContainerModel::has_image_info(self)
    }

    fn image_info(&self) -> Option<&ImageInspect> {
        ContainerModel::image_info(self)
    }

    fn get_lifecycle_pre_check_command(&self) -> String {
        ContainerModel::get_lifecycle_pre_check_command(self)
    }

    fn get_lifecycle_post_check_command(&self) -> String {
        ContainerModel::get_lifecycle_post_check_command(self)
    }

    fn get_lifecycle_pre_update_command(&self) -> String {
        ContainerModel::get_lifecycle_pre_update_command(self)
    }

    fn get_lifecycle_post_update_command(&self) -> String {
        ContainerModel::get_lifecycle_post_update_command(self)
    }

    fn verify_configuration(&mut self) -> Result<()> {
        ContainerModel::verify_configuration(self)
    }

    fn set_stale(&mut self, value: bool) {
        ContainerModel::set_stale(self, value);
    }

    fn is_stale(&self) -> bool {
        ContainerModel::is_stale(self)
    }

    fn is_no_pull(&self, params: &UpdateParams) -> bool {
        ContainerModel::is_no_pull(self, params)
    }

    fn set_linked_to_restarting(&mut self, value: bool) {
        ContainerModel::set_linked_to_restarting(self, value);
    }

    fn is_linked_to_restarting(&self) -> bool {
        ContainerModel::is_linked_to_restarting(self)
    }

    fn pre_update_timeout(&self) -> i64 {
        ContainerModel::pre_update_timeout(self)
    }

    fn post_update_timeout(&self) -> i64 {
        ContainerModel::post_update_timeout(self)
    }

    fn is_restarting(&self) -> bool {
        ContainerModel::is_restarting(self)
    }

    fn get_create_config(&mut self) -> Result<ContainerConfig> {
        ContainerModel::get_create_config(self)
    }

    fn get_create_host_config(&mut self) -> Result<HostConfig> {
        ContainerModel::get_create_host_config(self)
    }
}

/// Minimal runtime container surface used by pure action logic.
///
/// The trait mirrors the restart-oriented container state that Watchtower's
/// action layer needs without pulling in Docker-specific types.
pub trait RuntimeContainer {
    fn id(&self) -> &ContainerID;
    fn name(&self) -> &str;
    fn links(&self) -> &[String];
    fn image_id(&self) -> &ImageID;
    fn created_at(&self) -> &str;
    fn is_watchtower(&self) -> bool;
    fn is_stale(&self) -> bool;
    fn set_stale(&mut self, value: bool);
    fn is_linked_to_restarting(&self) -> bool;
    fn set_linked_to_restarting(&mut self, value: bool);
    fn is_monitor_only(&self, params: &UpdateParams) -> bool;

    fn to_restart(&self) -> bool {
        self.is_stale() || self.is_linked_to_restarting()
    }
}

fn short_id(id: &str) -> String {
    let mut offset = 0;
    let mut length = 12;

    if let Some(prefix_sep) = id.find(':') {
        if &id[..prefix_sep] == "sha256" {
            offset = prefix_sep + 1;
        } else {
            length += prefix_sep + 1;
        }
    }

    if id.len() >= offset + length {
        id[offset..offset + length].to_string()
    } else {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::filterable_container::FilterableContainer;

    struct MockContainer {
        id: ContainerID,
        name: String,
        links: Vec<String>,
        image_id: ImageID,
        watchtower: bool,
        stale: bool,
        linked_to_restarting: bool,
        monitor_only: bool,
        enabled: (bool, bool),
        scope: Option<String>,
        image_name: String,
    }

    impl FilterableContainer for MockContainer {
        fn name(&self) -> &str {
            &self.name
        }

        fn is_watchtower(&self) -> bool {
            self.watchtower
        }

        fn enabled(&self) -> (bool, bool) {
            self.enabled
        }

        fn scope(&self) -> Option<&str> {
            self.scope.as_deref()
        }

        fn image_name(&self) -> &str {
            &self.image_name
        }
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

        fn created_at(&self) -> &str {
            ""
        }

        fn is_watchtower(&self) -> bool {
            self.watchtower
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

    #[test]
    fn short_id_trims_prefix_and_length() {
        let id = ContainerID::from("sha256:1234567890abcdef");

        assert_eq!(id.short_id(), "1234567890ab");
        assert_eq!(ImageID::from("abcdef123456").short_id(), "abcdef123456");
    }

    #[test]
    fn short_id_preserves_non_sha_prefixes() {
        assert_eq!(
            ImageID::from("image:1234567890abcdef").short_id(),
            "image:1234567890ab"
        );
    }

    #[test]
    fn runtime_container_exposes_restart_state() {
        let mut container = MockContainer {
            id: ContainerID::from("abc123"),
            name: "app".to_string(),
            links: vec!["/db".to_string()],
            image_id: ImageID::from("sha256:image-b"),
            watchtower: false,
            stale: false,
            linked_to_restarting: false,
            monitor_only: false,
            enabled: (false, false),
            scope: None,
            image_name: "example/app:latest".to_string(),
        };

        assert_eq!(container.id().as_str(), "abc123");
        assert_eq!(container.links().first().map(String::as_str), Some("/db"));
        assert!(!container.to_restart());

        container.set_stale(true);
        assert!(container.is_stale());
        assert!(container.to_restart());

        container.set_stale(false);
        container.set_linked_to_restarting(true);
        assert!(container.is_linked_to_restarting());
        assert!(container.to_restart());
    }
}