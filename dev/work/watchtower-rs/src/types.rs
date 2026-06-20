//! Shared legacy watchtower types translated from:
//! - `old-source/pkg/types/container.go`
//! - `old-source/pkg/types/filter.go`
//! - `old-source/pkg/types/filterable_container.go`
//! - `old-source/pkg/types/notifier.go`
//! - `old-source/pkg/types/report.go`
//! - `old-source/pkg/types/update_params.go`

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::container::{
    Container as ContainerModel, ContainerConfig, ContainerInspect, HostConfig, ImageInspect,
};
use crate::Result;

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

/// A Filter is a prototype for a function that can be used to filter the
/// results from a call to the `ListContainers()` method on the `Client`.
pub type Filter = dyn Fn(&dyn FilterableContainer) -> bool + Send + Sync;

/// Minimal container filter surface used by the update pipeline.
pub trait FilterableContainer {
    fn name(&self) -> &str;
    fn is_watchtower(&self) -> bool;
    fn enabled(&self) -> (bool, bool);
    fn scope(&self) -> Option<&str>;
    fn image_name(&self) -> &str;
}

/// A notifier capable of creating a shoutrrr URL.
pub trait ConvertibleNotifier {
    fn get_url(
        &self,
        command: &clap::Command,
    ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync + 'static>>;
}

/// A notifier that might need to be delayed before sending notifications.
pub trait DelayNotifier {
    fn get_delay(&self) -> Duration;
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
        self.filter.as_ref().is_none_or(|filter| filter(container))
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
    /// Return the reported container ID.
    pub fn id(&self) -> &ContainerID {
        &self.id
    }

    /// Return the reported container name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Return the current image ID.
    pub fn current_image_id(&self) -> &ImageID {
        &self.current_image_id
    }

    /// Return the latest image ID.
    pub fn latest_image_id(&self) -> &ImageID {
        &self.latest_image_id
    }

    /// Return the image name associated with the report.
    pub fn image_name(&self) -> &str {
        self.image_name.as_str()
    }

    /// Return the recorded error text, or an empty string when the report has no error.
    pub fn error(&self) -> &str {
        self.error.as_deref().unwrap_or("")
    }

    /// Return the recorded state string.
    pub fn state(&self) -> &str {
        self.state.as_str()
    }

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
    /// Return the scanned entries in their recorded order.
    pub fn scanned(&self) -> &[ContainerReport] {
        &self.scanned
    }

    /// Return the updated entries in their recorded order.
    pub fn updated(&self) -> &[ContainerReport] {
        &self.updated
    }

    /// Return the failed entries in their recorded order.
    pub fn failed(&self) -> &[ContainerReport] {
        &self.failed
    }

    /// Return the skipped entries in their recorded order.
    pub fn skipped(&self) -> &[ContainerReport] {
        &self.skipped
    }

    /// Return the stale entries in their recorded order.
    pub fn stale(&self) -> &[ContainerReport] {
        &self.stale
    }

    /// Return the fresh entries in their recorded order.
    pub fn fresh(&self) -> &[ContainerReport] {
        &self.fresh
    }

    /// Return every recorded container entry once, in deterministic ID order.
    ///
    /// The legacy Go report deduplicated by container ID with the priority order
    /// `updated`, `failed`, `skipped`, `stale`, `fresh`, `scanned`.
    pub fn all(&self) -> Vec<ContainerReport> {
        self.all_refs().into_iter().cloned().collect()
    }

    /// Return borrowed views of every recorded container entry once, in
    /// deterministic ID order.
    pub fn all_refs(&self) -> Vec<&ContainerReport> {
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

/// Notification surface shared by the legacy notifier implementations.
pub trait Notifier {
    fn start_notification(&self);
    fn send_notification(&self, report: &Report);
    fn add_log_hook(&self);
    fn get_names(&self) -> Vec<String>;
    fn get_urls(&self) -> Vec<String>;
    fn close(&self);
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
    use std::cell::{Cell, RefCell};

    struct MockConvertibleNotifier;

    impl ConvertibleNotifier for MockConvertibleNotifier {
        fn get_url(
            &self,
            _command: &clap::Command,
        ) -> std::result::Result<String, Box<dyn std::error::Error + Send + Sync + 'static>> {
            Ok("slack://watchtower".to_string())
        }
    }

    struct MockDelayNotifier(Duration);

    impl DelayNotifier for MockDelayNotifier {
        fn get_delay(&self) -> Duration {
            self.0
        }
    }

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
    fn notifier_traits_preserve_legacy_contracts() {
        let notifier = MockConvertibleNotifier;
        let delay_notifier = MockDelayNotifier(Duration::from_secs(5));

        assert_eq!(
            notifier
                .get_url(&clap::Command::new("watchtower"))
                .expect("url should resolve"),
            "slack://watchtower"
        );
        assert_eq!(delay_notifier.get_delay(), Duration::from_secs(5));
    }

    #[test]
    fn short_id_preserves_non_sha_prefixes() {
        assert_eq!(
            ImageID::from("image:1234567890abcdef").short_id(),
            "image:1234567890ab"
        );
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
            image_id: ImageID::from("sha256:image-a"),
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
            .map(|entry| entry.id.to_string())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["a", "b", "c", "d", "e", "f", "g"]);
        assert!(!report.is_empty());
    }

    #[test]
    fn report_all_returns_owned_reports() {
        let report = Report {
            updated: vec![ContainerReport {
                id: ContainerID::from("a"),
                name: "name-a".to_string(),
                current_image_id: ImageID::from("old-a"),
                latest_image_id: ImageID::from("new-a"),
                image_name: "image-a".to_string(),
                error: None,
                state: "Updated".to_string(),
            }],
            ..Report::default()
        };

        let mut all = report.all();
        all[0].state = "Mutated".to_string();

        assert_eq!(report.updated()[0].state, "Updated");
        assert_eq!(report.all_refs()[0].state, "Updated");
    }

    #[test]
    fn container_report_accessors_match_legacy_shape() {
        let report = ContainerReport {
            id: ContainerID::from("container-id"),
            name: "name".to_string(),
            current_image_id: ImageID::from("old-image"),
            latest_image_id: ImageID::from("new-image"),
            image_name: "example/image:latest".to_string(),
            error: Some("boom".to_string()),
            state: "Failed".to_string(),
        };

        assert_eq!(report.id().as_str(), "container-id");
        assert_eq!(report.name(), "name");
        assert_eq!(report.current_image_id().as_str(), "old-image");
        assert_eq!(report.latest_image_id().as_str(), "new-image");
        assert_eq!(report.image_name(), "example/image:latest");
        assert_eq!(report.error(), "boom");
        assert_eq!(report.state(), "Failed");
        assert!(report.has_error());

        let no_error = ContainerReport {
            error: None,
            ..report
        };

        assert_eq!(no_error.error(), "");
        assert!(!no_error.has_error());
    }

    #[test]
    fn report_bucket_accessors_preserve_recorded_order() {
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
            scanned: vec![make("b", "Scanned"), make("a", "Scanned")],
            updated: vec![make("c", "Updated")],
            failed: vec![make("d", "Failed")],
            skipped: vec![make("e", "Skipped")],
            stale: vec![make("f", "Stale")],
            fresh: vec![make("g", "Fresh")],
        };

        assert_eq!(
            report
                .scanned()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert_eq!(
            report
                .updated()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
        assert_eq!(
            report
                .failed()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["d"]
        );
        assert_eq!(
            report
                .skipped()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["e"]
        );
        assert_eq!(
            report
                .stale()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["f"]
        );
        assert_eq!(
            report
                .fresh()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["g"]
        );
    }

    struct MockNotifier {
        started: Cell<bool>,
        hook_added: Cell<bool>,
        closed: Cell<bool>,
        names: Vec<String>,
        urls: Vec<String>,
        reports: RefCell<Vec<Report>>,
    }

    impl Notifier for MockNotifier {
        fn start_notification(&self) {
            self.started.set(true);
        }

        fn send_notification(&self, report: &Report) {
            self.reports.borrow_mut().push(report.clone());
        }

        fn add_log_hook(&self) {
            self.hook_added.set(true);
        }

        fn get_names(&self) -> Vec<String> {
            self.names.clone()
        }

        fn get_urls(&self) -> Vec<String> {
            self.urls.clone()
        }

        fn close(&self) {
            self.closed.set(true);
        }
    }

    #[test]
    fn notifier_trait_exposes_legacy_surface() {
        let notifier = MockNotifier {
            started: Cell::new(false),
            hook_added: Cell::new(false),
            closed: Cell::new(false),
            names: vec!["logger".to_string()],
            urls: vec!["stdout://".to_string()],
            reports: RefCell::new(Vec::new()),
        };
        let report = Report {
            scanned: vec![ContainerReport {
                id: ContainerID::from("a"),
                name: "name-a".to_string(),
                current_image_id: ImageID::from("old-a"),
                latest_image_id: ImageID::from("new-a"),
                image_name: "image-a".to_string(),
                error: None,
                state: "Scanned".to_string(),
            }],
            ..Report::default()
        };

        notifier.start_notification();
        notifier.send_notification(&report);
        notifier.add_log_hook();
        notifier.close();

        assert!(notifier.started.get());
        assert!(notifier.hook_added.get());
        assert!(notifier.closed.get());
        assert_eq!(notifier.get_names(), vec!["logger".to_string()]);
        assert_eq!(notifier.get_urls(), vec!["stdout://".to_string()]);
        assert_eq!(notifier.reports.borrow().as_slice(), &[report]);
    }
}
