use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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

/// A filter predicate used when selecting containers.
pub type Filter = dyn Fn(&dyn FilterableContainer) -> bool + Send + Sync;

/// Minimal container filter surface used by the update pipeline.
pub trait FilterableContainer {
    fn name(&self) -> &str;
    fn is_watchtower(&self) -> bool;
    fn enabled(&self) -> (bool, bool);
    fn scope(&self) -> Option<&str>;
    fn image_name(&self) -> &str;
}

/// Minimal runtime container surface used by pure action logic.
///
/// The trait mirrors the restart-oriented container state that Watchtower's
/// action layer needs without pulling in Docker-specific types.
pub trait RuntimeContainer {
    fn id(&self) -> &ContainerID;
    fn name(&self) -> &str;
    fn links(&self) -> &[String];
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

/// Parameters that control an update pass.
#[derive(Clone, Default)]
pub struct UpdateParams {
    pub filter: Option<Arc<Filter>>,
    pub cleanup: bool,
    pub no_restart: bool,
    pub timeout: Duration,
    pub monitor_only: bool,
    pub no_pull: bool,
    pub lifecycle_hooks: bool,
    pub rolling_restart: bool,
    pub label_precedence: bool,
}

impl UpdateParams {
    /// Build a new parameter set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a filter predicate.
    pub fn with_filter(
        mut self,
        filter: impl Fn(&dyn FilterableContainer) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.filter = Some(Arc::new(filter));
        self
    }

    /// Return true when the container passes the configured filter.
    pub fn matches(&self, container: &dyn FilterableContainer) -> bool {
        self.filter.as_ref().map_or(true, |filter| filter(container))
    }
}

impl fmt::Debug for UpdateParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateParams")
            .field("filter", &self.filter.as_ref().map(|_| "<filter>"))
            .field("cleanup", &self.cleanup)
            .field("no_restart", &self.no_restart)
            .field("timeout", &self.timeout)
            .field("monitor_only", &self.monitor_only)
            .field("no_pull", &self.no_pull)
            .field("lifecycle_hooks", &self.lifecycle_hooks)
            .field("rolling_restart", &self.rolling_restart)
            .field("label_precedence", &self.label_precedence)
            .finish()
    }
}

/// A single container entry in an update report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerReport {
    pub id: ContainerID,
    pub name: String,
    pub current_image_id: ImageID,
    pub latest_image_id: ImageID,
    pub image_name: String,
    pub error: Option<String>,
    pub state: String,
}

impl ContainerReport {
    /// True when the report recorded an error.
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Aggregated session report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub scanned: Vec<ContainerReport>,
    pub updated: Vec<ContainerReport>,
    pub failed: Vec<ContainerReport>,
    pub skipped: Vec<ContainerReport>,
    pub stale: Vec<ContainerReport>,
    pub fresh: Vec<ContainerReport>,
}

impl Report {
    /// Return every recorded container entry once, in deterministic ID order.
    ///
    /// The legacy Go report deduplicated by container ID with the priority order
    /// `updated`, `failed`, `skipped`, `stale`, `fresh`, `scanned`.
    pub fn all(&self) -> Vec<&ContainerReport> {
        let mut all = Vec::with_capacity(
            self.scanned.len()
                + self.updated.len()
                + self.failed.len()
                + self.skipped.len()
                + self.stale.len()
                + self.fresh.len(),
        );
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

        for bucket in [
            &self.updated,
            &self.failed,
            &self.skipped,
            &self.stale,
            &self.fresh,
            &self.scanned,
        ] {
            for report in bucket {
                if seen.insert(report.id.as_str()) {
                    all.push(report);
                }
            }
        }

        all.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        all
    }

    /// Return true when the report has no recorded entries.
    pub fn is_empty(&self) -> bool {
        self.all().is_empty()
    }
}

fn short_id(id: &str) -> String {
    let trimmed = id.strip_prefix("sha256:").unwrap_or(id);
    trimmed.chars().take(12).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockContainer {
        id: ContainerID,
        name: String,
        links: Vec<String>,
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
    fn update_params_matches_filter() {
        let params = UpdateParams::new().with_filter(|container| {
            container.name() == "watchtower" && !container.is_watchtower()
        });
        let container = MockContainer {
            id: ContainerID::from("abc123"),
            name: "watchtower".to_string(),
            links: vec![],
            watchtower: false,
            stale: false,
            linked_to_restarting: false,
            monitor_only: false,
            enabled: (true, true),
            scope: Some("default".to_string()),
            image_name: "containrrr/watchtower:latest".to_string(),
        };

        assert!(params.matches(&container));
        assert_eq!(container.enabled(), (true, true));
        assert_eq!(container.scope(), Some("default"));
        assert_eq!(container.image_name(), "containrrr/watchtower:latest");
    }

    #[test]
    fn runtime_container_exposes_restart_state() {
        let mut container = MockContainer {
            id: ContainerID::from("abc123"),
            name: "app".to_string(),
            links: vec!["/db".to_string()],
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

    #[test]
    fn report_all_deduplicates_by_priority_and_sorts_by_id() {
        let make = |id: &str, state: &str| ContainerReport {
            id: ContainerID::from(id),
            name: format!("name-{id}"),
            current_image_id: ImageID::from(format!("old-{id}")),
            latest_image_id: ImageID::from(format!("new-{id}")),
            image_name: format!("image-{id}"),
            error: None,
            state: state.to_string(),
        };

        let report = Report {
            scanned: vec![make("c", "Scanned"), make("a", "Scanned")],
            updated: vec![make("b", "Updated"), make("a", "Updated")],
            failed: vec![make("d", "Failed")],
            skipped: vec![make("e", "Skipped")],
            stale: vec![make("f", "Stale")],
            fresh: vec![make("g", "Fresh")],
        };

        let ids = report
            .all()
            .into_iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["a", "b", "c", "d", "e", "f", "g"]);
        assert!(!report.is_empty());
    }
}
