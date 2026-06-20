#![forbid(unsafe_code)]

//! Docker client helper snapshots translated from the legacy Go client layer.
//!
//! The actual Docker HTTP transport is not implemented here. This module keeps
//! the deterministic parts that can be exercised without a live daemon:
//! warning strategy selection and network alias normalization.

use std::collections::HashMap;

use crate::registry::trust;
use crate::types::FilterableContainer;

/// Strategy used when deciding whether a failed HEAD request should warn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningStrategy {
    Always,
    Never,
    Auto,
}

impl Default for WarningStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

/// Container-network endpoint snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkEndpoint {
    pub aliases: Vec<String>,
}

/// Networking configuration snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkingConfig {
    pub endpoints: HashMap<String, NetworkEndpoint>,
}

/// Return whether the Docker client should warn for a HEAD failure.
#[must_use]
pub fn warn_on_head_pull_failed(strategy: WarningStrategy, image_name: &str) -> bool {
    match strategy {
        WarningStrategy::Always => true,
        WarningStrategy::Never => false,
        WarningStrategy::Auto => trust::warn_on_api_consumption(image_name).unwrap_or(true),
    }
}

/// Return whether the Docker client should warn for a HEAD failure.
#[must_use]
pub fn warn_on_head_pull_failed_for_container(
    strategy: WarningStrategy,
    container: &impl FilterableContainer,
) -> bool {
    warn_on_head_pull_failed(strategy, container.image_name())
}

/// Normalize network aliases for recreation.
///
/// The legacy Go client removed the old container ID alias from each endpoint's
/// alias list before reusing the network config. That behavior is preserved
/// here.
#[must_use]
pub fn normalize_network_config(mut config: NetworkingConfig, container_id_short: &str) -> NetworkingConfig {
    for endpoint in config.endpoints.values_mut() {
        endpoint.aliases.retain(|alias| alias != container_id_short);
    }

    config
}

/// Return a network config containing only the first endpoint.
#[must_use]
pub fn simple_network_config(config: &NetworkingConfig) -> NetworkingConfig {
    let mut endpoints = HashMap::new();

    if let Some((name, endpoint)) = config.endpoints.iter().next() {
        endpoints.insert(name.clone(), endpoint.clone());
    }

    NetworkingConfig { endpoints }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestContainer {
        image_name: String,
    }

    impl FilterableContainer for TestContainer {
        fn name(&self) -> &str {
            "test"
        }

        fn is_watchtower(&self) -> bool {
            false
        }

        fn enabled(&self) -> (bool, bool) {
            (true, true)
        }

        fn scope(&self) -> Option<&str> {
            None
        }

        fn image_name(&self) -> &str {
            self.image_name.as_str()
        }
    }

    fn endpoint(aliases: &[&str]) -> NetworkEndpoint {
        NetworkEndpoint {
            aliases: aliases.iter().map(|alias| (*alias).to_string()).collect(),
        }
    }

    #[test]
    fn warning_strategy_matches_legacy_head_behavior() {
        let container = TestContainer {
            image_name: "docker.io/library/nginx:latest".to_string(),
        };

        assert!(warn_on_head_pull_failed(WarningStrategy::Always, "registry.example.com/team/app:latest"));
        assert!(!warn_on_head_pull_failed(WarningStrategy::Never, "ubuntu"));
        assert!(warn_on_head_pull_failed(WarningStrategy::Auto, "ghcr.io/watchtower/image:main"));
        assert!(warn_on_head_pull_failed_for_container(WarningStrategy::Auto, &container));
    }

    #[test]
    fn normalize_network_config_removes_container_id_aliases_only() {
        let mut endpoints = HashMap::new();
        endpoints.insert("bridge".to_string(), endpoint(&["abc123", "db", "redis"]));
        endpoints.insert("other".to_string(), endpoint(&["abc123", "cache"]));

        let config = NetworkingConfig { endpoints };
        let normalized = normalize_network_config(config, "abc123");

        assert_eq!(
            normalized.endpoints.get("bridge").unwrap().aliases,
            vec!["db".to_string(), "redis".to_string()]
        );
        assert_eq!(
            normalized.endpoints.get("other").unwrap().aliases,
            vec!["cache".to_string()]
        );
    }

    #[test]
    fn simple_network_config_keeps_only_the_first_endpoint() {
        let mut endpoints = HashMap::new();
        endpoints.insert("bridge".to_string(), endpoint(&["db"]));
        endpoints.insert("other".to_string(), endpoint(&["cache"]));

        let config = NetworkingConfig { endpoints };
        let simple = simple_network_config(&config);

        assert_eq!(simple.endpoints.len(), 1);
        let endpoint = simple.endpoints.values().next().unwrap();
        assert!(endpoint.aliases == vec!["db".to_string()] || endpoint.aliases == vec!["cache".to_string()]);
    }
}
