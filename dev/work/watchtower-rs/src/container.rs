#![forbid(unsafe_code)]
#![allow(dead_code)]
//! Docker container parity helpers translated from the legacy Go container package.
//!
//! The module keeps the data model small and explicit so the container-specific
//! logic can be exercised without wiring in Docker HTTP clients yet.

pub mod errors;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use crate::docker_client::{NetworkEndpoint, NetworkingConfig};
use crate::types::{ContainerID, FilterableContainer, ImageID, RuntimeContainer, UpdateParams};
use crate::{Error, Result};
use tracing::warn;

const WATCHTOWER_LABEL: &str = "com.centurylinklabs.watchtower";
const SIGNAL_LABEL: &str = "com.centurylinklabs.watchtower.stop-signal";
const ENABLE_LABEL: &str = "com.centurylinklabs.watchtower.enable";
const MONITOR_ONLY_LABEL: &str = "com.centurylinklabs.watchtower.monitor-only";
const NO_PULL_LABEL: &str = "com.centurylinklabs.watchtower.no-pull";
const DEPENDS_ON_LABEL: &str = "com.centurylinklabs.watchtower.depends-on";
const ZODIAC_LABEL: &str = "com.centurylinklabs.zodiac.original-image";
const SCOPE_LABEL: &str = "com.centurylinklabs.watchtower.scope";
const PRE_CHECK_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-check";
const POST_CHECK_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.post-check";
const PRE_UPDATE_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-update";
const POST_UPDATE_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.post-update";
const PRE_UPDATE_TIMEOUT_LABEL: &str =
    "com.centurylinklabs.watchtower.lifecycle.pre-update-timeout";
const POST_UPDATE_TIMEOUT_LABEL: &str =
    "com.centurylinklabs.watchtower.lifecycle.post-update-timeout";

fn error_no_image_info() -> Error {
    Error::InvalidConfig("no available image info".to_string())
}

fn error_no_container_info() -> Error {
    Error::InvalidConfig("no available container info".to_string())
}

fn error_invalid_config() -> Error {
    Error::InvalidConfig("container configuration missing or invalid".to_string())
}

fn error_label_not_found() -> BoolLabelError {
    BoolLabelError::NotFound
}

fn parse_bool_like_go(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "t" | "true" => Some(true),
        "0" | "f" | "false" => Some(false),
        _ => None,
    }
}

pub fn contains_watchtower_label(labels: &BTreeMap<String, String>) -> bool {
    labels
        .get(WATCHTOWER_LABEL)
        .is_some_and(|value| value == "true")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoolLabelError {
    NotFound,
    Invalid,
}

fn duration_or_zero(duration: Duration) -> Duration {
    duration
}

fn clear_if_equal<T: Default + PartialEq>(current: &mut T, default: &T) {
    if current == default {
        *current = T::default();
    }
}

/// Container-level state used for restart and health decisions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ContainerState {
    pub running: bool,
    pub restarting: bool,
}

/// A Docker network mode snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NetworkMode {
    #[default]
    Default,
    Host,
    Container(String),
    Other(String),
}

impl NetworkMode {
    pub fn is_container(&self) -> bool {
        matches!(self, Self::Container(_))
    }

    pub fn is_host(&self) -> bool {
        matches!(self, Self::Host)
    }

    pub fn connected_container(&self) -> Option<&str> {
        match self {
            Self::Container(name) => Some(name.as_str()),
            _ => None,
        }
    }
}

/// A port binding placeholder used by the host config model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PortBinding {
    pub host_ip: Option<String>,
    pub host_port: Option<String>,
}

/// A health check snapshot compatible with the legacy parity logic.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HealthConfig {
    pub test: Vec<String>,
    pub interval: Duration,
    pub timeout: Duration,
    pub start_period: Duration,
    pub retries: u32,
}

/// Runtime container config snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContainerConfig {
    pub image: String,
    pub labels: BTreeMap<String, String>,
    pub working_dir: String,
    pub user: String,
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    pub env: Vec<String>,
    pub volumes: BTreeSet<String>,
    pub exposed_ports: Option<BTreeSet<String>>,
    pub healthcheck: Option<HealthConfig>,
    pub hostname: String,
}

/// Runtime host config snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostConfig {
    pub links: Vec<String>,
    pub network_mode: NetworkMode,
    pub port_bindings: BTreeMap<String, Vec<PortBinding>>,
    pub auto_remove: bool,
}

/// Image inspection snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInspect {
    pub id: ImageID,
    pub config: ContainerConfig,
}

/// Container inspection snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInspect {
    pub id: ContainerID,
    pub name: String,
    pub image: ImageID,
    pub created: String,
    pub state: ContainerState,
    pub config: Option<ContainerConfig>,
    pub host_config: Option<HostConfig>,
    pub network_settings: Option<HashMap<String, NetworkEndpoint>>,
}

impl Default for ImageInspect {
    fn default() -> Self {
        Self {
            id: ImageID::new(""),
            config: ContainerConfig::default(),
        }
    }
}

impl Default for ContainerInspect {
    fn default() -> Self {
        Self {
            id: ContainerID::new(""),
            name: String::new(),
            image: ImageID::new(""),
            created: String::new(),
            state: ContainerState::default(),
            config: None,
            host_config: None,
            network_settings: None,
        }
    }
}

/// Container parity object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    container_info: Option<ContainerInspect>,
    image_info: Option<ImageInspect>,
    linked_to_restarting: bool,
    stale: bool,
    resolved_id: ContainerID,
    resolved_name: String,
    resolved_image_id: ImageID,
    resolved_image_name: String,
    resolved_links: Vec<String>,
}

impl Container {
    /// Build a new container wrapper from inspection snapshots.
    pub fn new(container_info: ContainerInspect, image_info: Option<ImageInspect>) -> Self {
        let mut container = Self {
            container_info: Some(container_info),
            image_info,
            linked_to_restarting: false,
            stale: false,
            resolved_id: ContainerID::new(""),
            resolved_name: String::new(),
            resolved_image_id: ImageID::new(""),
            resolved_image_name: String::new(),
            resolved_links: Vec::new(),
        };
        container.refresh_cache();
        container
    }

    fn refresh_cache(&mut self) {
        let container_info = self.container_info.as_ref();
        self.resolved_id = container_info
            .map(|info| info.id.clone())
            .unwrap_or_else(|| ContainerID::new(""));
        self.resolved_name = container_info
            .map(|info| info.name.clone())
            .unwrap_or_default();
        self.resolved_image_id = self
            .image_info
            .as_ref()
            .map(|info| info.id.clone())
            .or_else(|| container_info.map(|info| info.image.clone()))
            .unwrap_or_else(|| ImageID::new(""));
        self.resolved_image_name = self.resolve_image_name();
        self.resolved_links = self.resolve_links();
    }

    fn resolve_image_name(&self) -> String {
        let base = self
            .container_info
            .as_ref()
            .and_then(|info| info.config.as_ref())
            .and_then(|config| {
                config
                    .labels
                    .get(ZODIAC_LABEL)
                    .map(String::as_str)
                    .or(Some(config.image.as_str()))
            })
            .unwrap_or("");

        if base.contains(':') {
            base.to_string()
        } else {
            format!("{base}:latest")
        }
    }

    fn resolve_links(&self) -> Vec<String> {
        let mut links = Vec::new();

        let Some(container_info) = self.container_info.as_ref() else {
            return links;
        };

        let Some(config) = container_info.config.as_ref() else {
            return links;
        };

        if let Some(depends_on) = config.labels.get(DEPENDS_ON_LABEL) {
            if depends_on.is_empty() {
                return links;
            }

            for link in depends_on.split(',') {
                let normalized = if link.starts_with('/') {
                    link.to_string()
                } else {
                    format!("/{link}")
                };
                links.push(normalized);
            }
            return links;
        }

        if let Some(host_config) = container_info.host_config.as_ref() {
            for link in &host_config.links {
                let name = link.split_once(':').map(|(name, _)| name).unwrap_or(link);
                links.push(name.to_string());
            }

            if let Some(name) = host_config.network_mode.connected_container() {
                links.push(name.to_string());
            }
        }

        links
    }

    fn build_network_config(&self) -> NetworkingConfig {
        let mut endpoints = self
            .container_info
            .as_ref()
            .and_then(|info| info.network_settings.as_ref())
            .cloned()
            .unwrap_or_default();

        let container_id_short = self.id().short_id();
        for endpoint in endpoints.values_mut() {
            endpoint
                .aliases
                .retain(|alias| alias != &container_id_short);
        }

        NetworkingConfig {
            endpoints: endpoints.into_iter().collect(),
        }
    }

    fn get_label_value_or_empty(&self, label: &str) -> &str {
        self.get_label_value(label).unwrap_or("")
    }

    fn get_label_value<'a>(&'a self, label: &str) -> Option<&'a str> {
        self.container_info
            .as_ref()
            .and_then(|info| info.config.as_ref())
            .and_then(|config| config.labels.get(label).map(String::as_str))
    }

    fn get_bool_label_value(&self, label: &str) -> std::result::Result<bool, BoolLabelError> {
        let Some(value) = self.get_label_value(label) else {
            return Err(error_label_not_found());
        };

        parse_bool_like_go(value).ok_or(BoolLabelError::Invalid)
    }

    fn subtract_runtime_overrides(config: &mut ContainerConfig, image_config: &ContainerConfig) {
        config.env.retain(|value| !image_config.env.contains(value));
        config
            .labels
            .retain(|key, value| image_config.labels.get(key) != Some(value));
        config
            .volumes
            .retain(|value| !image_config.volumes.contains(value));
    }

    fn clear_simple_fields(
        config: &mut ContainerConfig,
        image_config: &ContainerConfig,
        host_config: &HostConfig,
    ) {
        if config.working_dir == image_config.working_dir {
            config.working_dir.clear();
        }
        if config.user == image_config.user {
            config.user.clear();
        }
        if host_config.network_mode.is_container() {
            config.hostname.clear();
        }
    }

    fn clear_entrypoint_cmd_if_default(
        config: &mut ContainerConfig,
        image_config: &ContainerConfig,
    ) {
        if config.entrypoint == image_config.entrypoint {
            config.entrypoint.clear();
            if config.cmd == image_config.cmd {
                config.cmd.clear();
            }
        }
    }

    fn clear_healthcheck_defaults(config: &mut ContainerConfig, image_config: &ContainerConfig) {
        let Some(current) = config.healthcheck.as_mut() else {
            return;
        };
        let Some(default) = image_config.healthcheck.as_ref() else {
            return;
        };

        if current.test == default.test {
            current.test.clear();
        }
        clear_if_equal(&mut current.retries, &default.retries);
        clear_if_equal(&mut current.interval, &default.interval);
        clear_if_equal(&mut current.timeout, &default.timeout);
        clear_if_equal(&mut current.start_period, &default.start_period);
    }

    fn adjust_ports(
        config: &mut ContainerConfig,
        image_config: &ContainerConfig,
        host_config: &HostConfig,
    ) {
        if let Some(exposed_ports) = config.exposed_ports.as_mut() {
            if let Some(image_ports) = image_config.exposed_ports.as_ref() {
                exposed_ports.retain(|port| !image_ports.contains(port));
            }

            for port in host_config.port_bindings.keys() {
                exposed_ports.insert(port.clone());
            }
        }
    }

    fn normalize_link(link: &str) -> String {
        let name = link.split_once(':').map(|(name, _)| name).unwrap_or(link);
        let alias = match link.rfind('/') {
            Some(index) => &link[index..],
            None => link,
        };
        format!("{name}:{alias}")
    }

    /// Return the current container info snapshot.
    pub fn container_info(&self) -> Option<&ContainerInspect> {
        self.container_info.as_ref()
    }

    /// Return the current image info snapshot.
    pub fn image_info(&self) -> Option<&ImageInspect> {
        self.image_info.as_ref()
    }

    /// Return the cached container ID, or an empty value when inspection data is missing.
    pub fn id(&self) -> &ContainerID {
        &self.resolved_id
    }

    /// Return the cached container name, or an empty value when inspection data is missing.
    pub fn name(&self) -> &str {
        self.resolved_name.as_str()
    }

    /// Return the cached image ID, or an empty value when inspection data is missing.
    pub fn image_id(&self) -> &ImageID {
        &self.resolved_image_id
    }

    /// Return the container creation timestamp if available.
    pub fn created_at(&self) -> &str {
        self.container_info
            .as_ref()
            .map(|info| info.created.as_str())
            .unwrap_or("")
    }

    /// Return the image ID if image inspection data is available.
    pub fn safe_image_id(&self) -> Option<&ImageID> {
        self.image_info.as_ref().map(|info| &info.id)
    }

    /// Return the resolved image name used for recreating the container.
    pub fn image_name(&self) -> &str {
        self.resolved_image_name.as_str()
    }

    /// Return the resolved link list.
    pub fn links(&self) -> &[String] {
        self.resolved_links.as_slice()
    }

    /// Return the network configuration used when recreating the container.
    pub fn get_network_config(&self) -> NetworkingConfig {
        self.build_network_config()
    }

    /// Return whether the container is running.
    pub fn is_running(&self) -> bool {
        self.container_info
            .as_ref()
            .is_some_and(|info| info.state.running)
    }

    /// Return whether the container is restarting.
    pub fn is_restarting(&self) -> bool {
        self.container_info
            .as_ref()
            .is_some_and(|info| info.state.restarting)
    }

    /// Return whether image metadata is available.
    pub fn has_image_info(&self) -> bool {
        self.image_info.is_some()
    }

    /// Return the enabled label as a parsed bool plus presence flag.
    pub fn enabled(&self) -> (bool, bool) {
        let Some(raw) = self.get_label_value(ENABLE_LABEL) else {
            return (false, false);
        };

        let Some(parsed) = parse_bool_like_go(raw) else {
            return (false, false);
        };

        (parsed, true)
    }

    /// Return the container scope if it exists.
    pub fn scope(&self) -> Option<&str> {
        self.get_label_value(SCOPE_LABEL)
    }

    /// Return the custom stop signal, or an empty string.
    pub fn stop_signal(&self) -> String {
        self.get_label_value_or_empty(SIGNAL_LABEL).to_string()
    }

    /// Return whether this is the watchtower container itself.
    pub fn is_watchtower(&self) -> bool {
        self.container_info
            .as_ref()
            .and_then(|info| info.config.as_ref())
            .is_some_and(|config| contains_watchtower_label(&config.labels))
    }

    /// Return the lifecycle pre-check command.
    pub fn get_lifecycle_pre_check_command(&self) -> String {
        self.get_label_value_or_empty(PRE_CHECK_LABEL).to_string()
    }

    /// Return the lifecycle post-check command.
    pub fn get_lifecycle_post_check_command(&self) -> String {
        self.get_label_value_or_empty(POST_CHECK_LABEL).to_string()
    }

    /// Return the lifecycle pre-update command.
    pub fn get_lifecycle_pre_update_command(&self) -> String {
        self.get_label_value_or_empty(PRE_UPDATE_LABEL).to_string()
    }

    /// Return the lifecycle post-update command.
    pub fn get_lifecycle_post_update_command(&self) -> String {
        self.get_label_value_or_empty(POST_UPDATE_LABEL).to_string()
    }

    /// Return whether a container update should only be monitored.
    pub fn is_monitor_only(&self, params: &UpdateParams) -> bool {
        self.get_container_or_global_bool(
            params.monitor_only,
            MONITOR_ONLY_LABEL,
            params.label_precedence,
        )
    }

    /// Return whether the container image should not be pulled.
    pub fn is_no_pull(&self, params: &UpdateParams) -> bool {
        self.get_container_or_global_bool(params.no_pull, NO_PULL_LABEL, params.label_precedence)
    }

    fn get_container_or_global_bool(
        &self,
        global_value: bool,
        label: &str,
        label_precedence: bool,
    ) -> bool {
        match self.get_bool_label_value(label) {
            Ok(container_value) => {
                if label_precedence {
                    container_value
                } else {
                    container_value || global_value
                }
            }
            Err(BoolLabelError::NotFound) => global_value,
            Err(BoolLabelError::Invalid) => {
                warn!(label, "Failed to parse label value");
                global_value
            }
        }
    }

    /// Return whether the container should be restarted.
    pub fn to_restart(&self) -> bool {
        self.stale || self.linked_to_restarting
    }

    /// Return the pre-update timeout in minutes.
    pub fn pre_update_timeout(&self) -> i64 {
        let value = self.get_label_value_or_empty(PRE_UPDATE_TIMEOUT_LABEL);
        value.parse::<i64>().unwrap_or(1)
    }

    /// Return the post-update timeout in minutes.
    pub fn post_update_timeout(&self) -> i64 {
        let value = self.get_label_value_or_empty(POST_UPDATE_TIMEOUT_LABEL);
        value.parse::<i64>().unwrap_or(1)
    }

    /// Set the stale flag.
    pub fn set_stale(&mut self, value: bool) {
        self.stale = value;
    }

    /// Return the stale flag.
    pub fn is_stale(&self) -> bool {
        self.stale
    }

    /// Set the linked-to-restarting flag.
    pub fn set_linked_to_restarting(&mut self, value: bool) {
        self.linked_to_restarting = value;
    }

    /// Return the linked-to-restarting flag.
    pub fn is_linked_to_restarting(&self) -> bool {
        self.linked_to_restarting
    }

    /// Return a create config with runtime-only overrides removed.
    pub fn get_create_config(&mut self) -> Result<ContainerConfig> {
        let image_name = self.image_name().to_string();
        let image_config = self
            .image_info
            .as_ref()
            .ok_or_else(error_no_image_info)?
            .config
            .clone();

        let host_config = self
            .container_info
            .as_ref()
            .and_then(|info| info.host_config.as_ref())
            .cloned()
            .ok_or_else(error_invalid_config)?;

        let container_info = self
            .container_info
            .as_mut()
            .ok_or_else(error_no_container_info)?;
        let container_config = container_info
            .config
            .as_mut()
            .ok_or_else(error_invalid_config)?;

        Self::clear_simple_fields(container_config, &image_config, &host_config);
        Self::clear_entrypoint_cmd_if_default(container_config, &image_config);
        Self::clear_healthcheck_defaults(container_config, &image_config);
        Self::subtract_runtime_overrides(container_config, &image_config);
        Self::adjust_ports(container_config, &image_config, &host_config);
        container_config.image = image_name;

        let config = container_config.clone();
        self.refresh_cache();
        Ok(config)
    }

    /// Return a create host config with links normalized for recreation.
    pub fn get_create_host_config(&mut self) -> Result<HostConfig> {
        let container_info = self
            .container_info
            .as_mut()
            .ok_or_else(error_no_container_info)?;
        let host_config = container_info
            .host_config
            .as_mut()
            .ok_or_else(error_invalid_config)?;

        for link in &mut host_config.links {
            *link = Self::normalize_link(link);
        }

        let host_config = host_config.clone();
        self.refresh_cache();
        Ok(host_config)
    }

    /// Check whether the container and image configuration are usable.
    pub fn verify_configuration(&mut self) -> Result<()> {
        if self.image_info.is_none() {
            return Err(error_no_image_info());
        }

        let container_info = self
            .container_info
            .as_mut()
            .ok_or_else(error_no_container_info)?;

        let container_config = container_info
            .config
            .as_mut()
            .ok_or_else(error_invalid_config)?;
        let host_config = container_info
            .host_config
            .as_mut()
            .ok_or_else(error_invalid_config)?;

        if !host_config.port_bindings.is_empty() && container_config.exposed_ports.is_none() {
            container_config.exposed_ports = Some(BTreeSet::new());
        }

        self.refresh_cache();
        Ok(())
    }
}

impl FilterableContainer for Container {
    fn name(&self) -> &str {
        self.name()
    }

    fn is_watchtower(&self) -> bool {
        self.is_watchtower()
    }

    fn enabled(&self) -> (bool, bool) {
        self.enabled()
    }

    fn scope(&self) -> Option<&str> {
        self.scope()
    }

    fn image_name(&self) -> &str {
        self.image_name()
    }
}

impl crate::filters::FilterableContainer for Container {
    fn name(&self) -> &str {
        self.name()
    }

    fn is_watchtower(&self) -> bool {
        self.is_watchtower()
    }

    fn enabled(&self) -> (bool, bool) {
        self.enabled()
    }

    fn scope(&self) -> Option<&str> {
        self.scope()
    }

    fn image_name(&self) -> &str {
        self.image_name()
    }
}

#[cfg(test)]
impl crate::sorter::SortableContainer for Container {
    fn name(&self) -> &str {
        self.name()
    }

    fn links(&self) -> &[String] {
        self.links()
    }
}

impl crate::session::ContainerLike for Container {
    fn id(&self) -> &ContainerID {
        self.id()
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn image_name(&self) -> &str {
        self.image_name()
    }

    fn current_image_id(&self) -> &ImageID {
        self.image_id()
    }
}

impl RuntimeContainer for Container {
    fn id(&self) -> &ContainerID {
        self.id()
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn links(&self) -> &[String] {
        self.links()
    }

    fn image_id(&self) -> &ImageID {
        self.image_id()
    }

    fn created_at(&self) -> &str {
        self.created_at()
    }

    fn is_watchtower(&self) -> bool {
        self.is_watchtower()
    }

    fn is_stale(&self) -> bool {
        self.is_stale()
    }

    fn set_stale(&mut self, value: bool) {
        self.set_stale(value);
    }

    fn is_linked_to_restarting(&self) -> bool {
        self.is_linked_to_restarting()
    }

    fn set_linked_to_restarting(&mut self, value: bool) {
        self.set_linked_to_restarting(value);
    }

    fn is_monitor_only(&self, params: &UpdateParams) -> bool {
        self.is_monitor_only(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    fn base_container() -> Container {
        let container_info = ContainerInspect {
            id: ContainerID::from("container_id"),
            name: "test-containrrr".to_string(),
            image: ImageID::from("sha256:current"),
            created: "2024-06-18T12:00:00Z".to_string(),
            state: ContainerState {
                running: true,
                restarting: false,
            },
            config: Some(ContainerConfig {
                image: "image-name".to_string(),
                labels: labels(&[
                    (ENABLE_LABEL, "true"),
                    (WATCHTOWER_LABEL, "true"),
                    (SCOPE_LABEL, "prod"),
                ]),
                working_dir: "/work".to_string(),
                user: "root".to_string(),
                entrypoint: vec!["/bin/sh".to_string(), "-c".to_string()],
                cmd: vec!["echo".to_string(), "hello".to_string()],
                env: vec!["KEEP=yes".to_string(), "REMOVE=no".to_string()],
                volumes: BTreeSet::from(["/data".to_string()]),
                exposed_ports: Some(BTreeSet::from(["80/tcp".to_string()])),
                healthcheck: Some(HealthConfig {
                    test: vec!["CMD-SHELL".to_string(), "curl -f localhost".to_string()],
                    interval: Duration::from_secs(10),
                    timeout: Duration::from_secs(5),
                    start_period: Duration::from_secs(3),
                    retries: 2,
                }),
                hostname: "old-host".to_string(),
            }),
            host_config: Some(HostConfig {
                links: vec!["/redis:/test-containrrr".to_string()],
                network_mode: NetworkMode::Default,
                port_bindings: BTreeMap::from([
                    ("80/tcp".to_string(), vec![PortBinding::default()]),
                    ("443/tcp".to_string(), vec![PortBinding::default()]),
                ]),
                auto_remove: false,
            }),
            network_settings: Some(HashMap::from([(
                "bridge".to_string(),
                NetworkEndpoint {
                    aliases: vec![
                        "container_id".to_string(),
                        "db".to_string(),
                        "redis".to_string(),
                    ],
                },
            )])),
        };

        let image_info = ImageInspect {
            id: ImageID::from("sha256:image"),
            config: ContainerConfig {
                image: "image-name:latest".to_string(),
                labels: labels(&[(ENABLE_LABEL, "false"), ("keep", "me")]),
                working_dir: "/work".to_string(),
                user: "root".to_string(),
                entrypoint: vec!["/bin/sh".to_string(), "-c".to_string()],
                cmd: vec!["echo".to_string(), "hello".to_string()],
                env: vec!["REMOVE=no".to_string()],
                volumes: BTreeSet::from(["/cache".to_string()]),
                exposed_ports: Some(BTreeSet::from([
                    "80/tcp".to_string(),
                    "443/tcp".to_string(),
                ])),
                healthcheck: Some(HealthConfig {
                    test: vec!["CMD-SHELL".to_string(), "curl -f localhost".to_string()],
                    interval: Duration::from_secs(10),
                    timeout: Duration::from_secs(5),
                    start_period: Duration::from_secs(3),
                    retries: 2,
                }),
                hostname: "image-host".to_string(),
            },
        };

        Container::new(container_info, Some(image_info))
    }

    #[test]
    fn label_access_uses_go_style_parsing() {
        let c = base_container();

        assert_eq!(c.enabled(), (true, true));
        assert!(c.is_watchtower());
        assert_eq!(c.scope(), Some("prod"));
        assert_eq!(c.stop_signal(), "");
        assert_eq!(c.get_lifecycle_pre_check_command(), "");
        assert_eq!(c.get_lifecycle_post_check_command(), "");
    }

    #[test]
    fn restart_and_link_logic_matches_label_and_host_config_rules() {
        let mut c = base_container();
        assert_eq!(c.links(), &["/redis".to_string()]);
        assert!(!c.to_restart());

        c.set_stale(true);
        assert!(c.to_restart());

        let mut linked = c.clone();
        linked.set_stale(false);
        linked.set_linked_to_restarting(true);
        assert!(linked.to_restart());

        let container_depends_on = Container::new(
            ContainerInspect {
                created: "2024-06-18T12:00:00Z".to_string(),
                config: Some(ContainerConfig {
                    labels: labels(&[(DEPENDS_ON_LABEL, "postgres,redis")]),
                    ..ContainerConfig::default()
                }),
                ..ContainerInspect::default()
            },
            None,
        );
        assert_eq!(
            container_depends_on.links(),
            &["/postgres".to_string(), "/redis".to_string()]
        );
    }

    #[test]
    fn get_network_config_strips_the_container_id_alias() {
        let c = base_container();
        let network_config = c.get_network_config();

        let endpoint = network_config
            .endpoints
            .get("bridge")
            .expect("bridge endpoint");
        assert_eq!(
            endpoint.aliases,
            vec!["db".to_string(), "redis".to_string()]
        );
    }

    #[test]
    fn create_config_strips_default_and_runtime_overrides() {
        let mut c = base_container();
        let config = c.get_create_config().expect("create config");

        assert_eq!(config.image, "image-name:latest");
        assert!(config.working_dir.is_empty());
        assert!(config.user.is_empty());
        assert!(config.entrypoint.is_empty());
        assert!(config.cmd.is_empty());
        assert_eq!(config.env, vec!["KEEP=yes".to_string()]);
        assert!(config.labels.contains_key(WATCHTOWER_LABEL));
        assert!(!config.labels.contains_key("keep"));
        assert_eq!(
            config.healthcheck,
            Some(HealthConfig {
                test: vec![],
                interval: Duration::ZERO,
                timeout: Duration::ZERO,
                start_period: Duration::ZERO,
                retries: 0,
            })
        );
        assert_eq!(
            config.exposed_ports,
            Some(BTreeSet::from([
                "80/tcp".to_string(),
                "443/tcp".to_string()
            ]))
        );
    }

    #[test]
    fn host_config_rewrites_links_for_recreation() {
        let mut c = base_container();
        let host_config = c.get_create_host_config().expect("host config");

        assert_eq!(
            host_config.links,
            vec!["/redis:/test-containrrr".to_string()]
        );
    }

    #[test]
    fn verify_configuration_repairs_missing_exposed_ports() {
        let mut c = base_container();
        {
            let container_info = c.container_info.as_mut().expect("container info");
            container_info
                .config
                .as_mut()
                .expect("config")
                .exposed_ports = None;
        }

        c.verify_configuration().expect("verify");

        let exposed = c
            .container_info()
            .and_then(|info| info.config.as_ref())
            .and_then(|config| config.exposed_ports.as_ref())
            .expect("exposed ports");
        assert!(exposed.is_empty());
    }

    #[test]
    fn verify_configuration_rejects_missing_parts() {
        let mut missing_image = base_container();
        missing_image.image_info = None;
        assert_eq!(
            missing_image.verify_configuration().expect_err("error"),
            error_no_image_info()
        );

        let mut missing_container = base_container();
        missing_container.container_info = None;
        missing_container.refresh_cache();
        assert_eq!(
            missing_container.verify_configuration().expect_err("error"),
            error_no_container_info()
        );
    }

    #[test]
    fn image_id_prefers_image_inspect_id_when_available() {
        let c = base_container();
        assert_eq!(c.image_id(), &ImageID::from("sha256:image"));
        assert_eq!(c.safe_image_id(), Some(&ImageID::from("sha256:image")));
    }

    #[test]
    fn image_name_uses_zodiac_label_and_appends_latest_when_missing_tag() {
        let c = Container::new(
            ContainerInspect {
                config: Some(ContainerConfig {
                    image: "ignored".to_string(),
                    labels: labels(&[(ZODIAC_LABEL, "the-original-image")]),
                    ..ContainerConfig::default()
                }),
                created: "2024-06-18T12:00:00Z".to_string(),
                ..ContainerInspect::default()
            },
            None,
        );

        assert_eq!(c.image_name(), "the-original-image:latest");
    }

    #[test]
    fn invalid_bool_label_falls_back_to_global_value() {
        let c = Container::new(
            ContainerInspect {
                config: Some(ContainerConfig {
                    labels: labels(&[(MONITOR_ONLY_LABEL, "maybe")]),
                    ..ContainerConfig::default()
                }),
                created: "2024-06-18T12:00:00Z".to_string(),
                ..ContainerInspect::default()
            },
            None,
        );
        let params = UpdateParams {
            monitor_only: true,
            ..UpdateParams::default()
        };

        assert!(c.is_monitor_only(&params));
    }
}
