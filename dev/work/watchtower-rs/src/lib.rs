#![forbid(unsafe_code)]
extern crate self as watchtower_rs;

// Library entrypoint for the Watchtower rewrite.
//
// This crate keeps the public surface focused on application configuration,
// validation, and the single orchestration entrypoint used by the binary.

use std::error::Error as StdError;
use std::fmt;
use std::thread;
use std::time::Duration;

use crate::container::Container;
use crate::docker_client::DockerCliAdapter;

pub mod actions;
pub mod api;
pub mod api_metrics;
pub mod api_update;
pub mod cgroup;
pub mod cli;
pub mod container;
pub mod docker_client;
pub mod filters;
pub mod flags;
pub mod lifecycle;
pub mod meta;
pub mod metrics;
pub mod notifications;
pub mod notifier;
pub mod notify_upgrade;
pub mod rand_name;
pub mod rand_sha256;
pub mod registry;
pub mod session;
pub mod sorter;
pub mod startup;
pub mod types;
pub mod util;
pub mod wait;

/// Shared result type for the library.
pub type Result<T> = std::result::Result<T, Error>;

/// Minimal error type for the initial skeleton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The current configuration is not usable.
    InvalidConfig(String),
    /// The health-check flag was passed to the main process.
    HealthCheckOnMainProcess,
    /// A runtime-dependent root phase cannot proceed yet.
    RuntimeAdapterMissing { phase: RootPhase, detail: String },
    /// The HTTP API wiring itself is invalid.
    HttpApi(crate::api::ApiError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(f, "invalid config: {message}"),
            Self::HealthCheckOnMainProcess => f.write_str(
                "The health check flag should never be passed to the main watchtower container process",
            ),
            Self::RuntimeAdapterMissing { phase, detail } => {
                write!(f, "root orchestration blocked in {phase}: {detail}")
            }
            Self::HttpApi(err) => write!(f, "http api error: {err}"),
        }
    }
}

impl StdError for Error {}

impl From<crate::api::ApiError> for Error {
    fn from(value: crate::api::ApiError) -> Self {
        Self::HttpApi(value)
    }
}

/// Ordered root phases mirrored from the legacy Go entrypoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootPhase {
    ConfigValidation,
    HealthCheck,
    FilterResolution,
    HttpApiWiring,
    AwaitDockerClient,
    SanityCheck,
    StartupEmission,
    RunOnceUpdate,
    MultipleInstanceProtection,
    HttpApiStart,
    SchedulerLoop,
    ShutdownWiring,
}

impl fmt::Display for RootPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::ConfigValidation => "config validation",
            Self::HealthCheck => "health check",
            Self::FilterResolution => "filter resolution",
            Self::HttpApiWiring => "http api wiring",
            Self::AwaitDockerClient => "docker client warmup",
            Self::SanityCheck => "sanity check",
            Self::StartupEmission => "startup emission",
            Self::RunOnceUpdate => "run once update",
            Self::MultipleInstanceProtection => "multiple instance protection",
            Self::HttpApiStart => "http api start",
            Self::SchedulerLoop => "scheduler loop",
            Self::ShutdownWiring => "shutdown wiring",
        };

        f.write_str(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RootBranch {
    RunOnce,
    HttpApiOnly,
    Scheduled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpApiWiringPlan {
    routes: Vec<&'static str>,
    start_decision: Option<std::result::Result<crate::api::StartDecision, crate::api::ApiError>>,
    blocker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SharedRuntimeWiring {
    update_lock_shared_between_scheduler_and_api: bool,
    shutdown_waits_for_running_update: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootExecutionPlan {
    filter_description: String,
    startup_config: AppConfig,
    branch: RootBranch,
    http_api: HttpApiWiringPlan,
    shared_runtime: SharedRuntimeWiring,
}

/// Inputs for the application.
///
/// This is the target shape for the CLI parser output. The binary can build it
/// directly or convert a parser struct into it with `Into<AppConfig>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {
    /// Positional container names that should be considered by the filter.
    pub containers: Vec<String>,
    /// Container names that should be excluded by the filter.
    pub disable_containers: Vec<String>,
    /// Enable label-based inclusion filtering.
    pub label_enable: bool,
    /// Run a single update pass and exit.
    pub run_once: bool,
    /// Only monitor containers and skip container updates.
    pub monitor_only: bool,
    /// Remove old images after a successful restart.
    pub cleanup: bool,
    /// Do not restart containers after a successful update.
    pub no_restart: bool,
    /// Stop timeout used when replacing containers.
    pub timeout: Duration,
    /// Remove anonymous volumes during container replacement.
    pub remove_volumes: bool,
    /// Include stopped containers in the scan.
    pub include_stopped: bool,
    /// Restart stopped containers that were updated.
    pub revive_stopped: bool,
    /// Include restarting containers in the scan.
    pub include_restarting: bool,
    /// Enable rolling restarts during updates.
    pub rolling_restart: bool,
    /// Accept a cron-style schedule instead of a fixed poll interval.
    pub schedule: Option<String>,
    /// Poll interval used when no schedule is set.
    pub interval: Option<Duration>,
    /// Do not pull any new images.
    pub no_pull: bool,
    /// Enable lifecycle hooks around update steps.
    pub lifecycle_hooks: bool,
    /// Allow labels to override the global command-line behavior.
    pub label_precedence: bool,
    /// Legacy HEAD pull warning strategy.
    pub warn_on_head_failure: Option<String>,
    /// Token for the HTTP API, when enabled.
    pub http_api_token: Option<String>,
    /// Notification transport names used by startup reporting.
    pub notification_types: Vec<String>,
    /// Enable the HTTP update endpoint.
    pub enable_http_update_api: bool,
    /// Enable the HTTP metrics endpoint.
    pub enable_http_metrics_api: bool,
    /// Allow the HTTP API to unblock periodic polls.
    pub unblock_http_api: bool,
    /// Optional container scope.
    pub scope: Option<String>,
    /// Skip the standard health check path and exit immediately.
    pub health_check: bool,
    /// Prevent the startup message from being emitted.
    pub no_startup_message: bool,
    /// Whether trace logging is enabled.
    pub trace_enabled: bool,
}

impl AppConfig {
    /// Build a config from CLI parser output or an already-normalized config.
    pub fn from_cli(config: impl Into<Self>) -> Self {
        config.into()
    }

    /// Validate obvious startup mistakes before orchestration starts.
    pub fn validate(&self) -> Result<()> {
        self.validate_schedule_and_interval()?;
        self.validate_container_flags()?;
        self.validate_runtime_flags()?;
        Ok(())
    }

    fn validate_schedule_and_interval(&self) -> Result<()> {
        if let Some(schedule) = self.schedule.as_deref() {
            if schedule.trim().is_empty() {
                return Err(Error::InvalidConfig(
                    "schedule must not be empty".to_string(),
                ));
            }
        }

        if matches!(self.interval, Some(interval) if interval.is_zero()) {
            return Err(Error::InvalidConfig(
                "interval must be greater than zero".to_string(),
            ));
        }

        if self.schedule.is_some() && self.interval.is_some() {
            return Err(Error::InvalidConfig(
                "schedule and interval are mutually exclusive".to_string(),
            ));
        }

        Ok(())
    }

    fn validate_container_flags(&self) -> Result<()> {
        if self.rolling_restart && self.monitor_only {
            return Err(Error::InvalidConfig(
                "rolling_restart cannot be combined with monitor_only".to_string(),
            ));
        }

        if self.revive_stopped && !self.include_stopped {
            return Err(Error::InvalidConfig(
                "revive_stopped requires include_stopped".to_string(),
            ));
        }

        if let Some(scope) = self.scope.as_deref() {
            if scope.trim().is_empty() {
                return Err(Error::InvalidConfig("scope must not be empty".to_string()));
            }
        }

        Ok(())
    }

    fn validate_runtime_flags(&self) -> Result<()> {
        if let Some(token) = self.http_api_token.as_deref() {
            if token.trim().is_empty() {
                return Err(Error::InvalidConfig(
                    "http_api_token must not be empty".to_string(),
                ));
            }
        }

        Ok(())
    }
}

/// In-memory application handle.
///
/// The binary can construct this from parsed CLI/config data and then call the
/// convenience [`run`] function below.
#[derive(Debug, Clone)]
pub struct WatchtowerApp {
    config: AppConfig,
}

impl WatchtowerApp {
    /// Create a new application instance from configuration.
    pub fn new(config: impl Into<AppConfig>) -> Self {
        Self {
            config: config.into(),
        }
    }

    pub fn run(&self) -> Result<()> {
        self.run_configuration_validation_phase()?;
        if self.run_health_check_phase()? {
            return Ok(());
        }

        let plan = self.prepare_root_execution_plan();
        self.run_http_api_wiring_phase(&plan)?;
        self.await_docker_client_phase();
        self.run_sanity_check_phase(&plan)?;
        self.emit_startup_phase(&plan);

        match plan.branch {
            RootBranch::RunOnce => self.run_once_branch(&plan),
            RootBranch::HttpApiOnly => self.http_api_only_branch(&plan),
            RootBranch::Scheduled => self.scheduled_branch(&plan),
        }
    }

    fn run_configuration_validation_phase(&self) -> Result<()> {
        self.config.validate()
    }

    fn run_health_check_phase(&self) -> Result<bool> {
        self.run_health_check_phase_with_pid(std::process::id())
    }

    fn run_health_check_phase_with_pid(&self, pid: u32) -> Result<bool> {
        if !self.config.health_check {
            return Ok(false);
        }

        if pid == 1 {
            thread::sleep(Duration::from_secs(1));
            return Err(Error::HealthCheckOnMainProcess);
        }

        Ok(true)
    }

    fn prepare_root_execution_plan(&self) -> RootExecutionPlan {
        let filter_description = filters::build_filter_description(
            &self.config.containers,
            &self.config.disable_containers,
            self.config.label_enable,
            self.config.scope.as_deref().unwrap_or(""),
        );
        let http_api = self.prepare_http_api_wiring();
        let branch = self.resolve_root_branch(&http_api);
        let startup_config = self.startup_config_for(&branch);

        RootExecutionPlan {
            filter_description,
            startup_config,
            branch,
            http_api,
            shared_runtime: SharedRuntimeWiring {
                update_lock_shared_between_scheduler_and_api: !self.config.run_once,
                shutdown_waits_for_running_update: !self.config.run_once,
            },
        }
    }

    fn prepare_http_api_wiring(&self) -> HttpApiWiringPlan {
        let mut api = crate::api::Api::new(
            self.config
                .http_api_token
                .as_deref()
                .unwrap_or_default()
                .to_string(),
        );
        let mut routes = Vec::new();
        let mut blocker = None;

        if self.config.enable_http_update_api {
            routes.push(crate::api_update::PATH);
            blocker = Some(self.update_api_runtime_blocker_detail());
        }

        if self.config.enable_http_metrics_api {
            routes.push(crate::api_metrics::PATH);
            self.register_metrics_http_route(&mut api);
        }

        let start_decision = if routes.is_empty() || blocker.is_some() {
            None
        } else {
            Some(api.start_decision(
                self.config.enable_http_update_api && !self.config.unblock_http_api,
            ))
        };

        HttpApiWiringPlan {
            routes,
            start_decision,
            blocker,
        }
    }

    fn resolve_root_branch(&self, _: &HttpApiWiringPlan) -> RootBranch {
        if self.config.run_once {
            return RootBranch::RunOnce;
        }

        if self.config.enable_http_update_api && !self.config.unblock_http_api {
            return RootBranch::HttpApiOnly;
        }

        RootBranch::Scheduled
    }

    fn startup_config_for(&self, branch: &RootBranch) -> AppConfig {
        let mut startup_config = self.config.clone();

        match branch {
            RootBranch::RunOnce => {
                startup_config.schedule = None;
                startup_config.interval = None;
                startup_config.run_once = true;
            }
            RootBranch::HttpApiOnly => {
                startup_config.schedule = None;
                startup_config.interval = None;
                startup_config.run_once = false;
            }
            RootBranch::Scheduled => {}
        }

        startup_config
    }

    fn await_docker_client_phase(&self) {
        tracing::debug!(
            "Sleeping for a second to ensure the docker api client has been properly initialized."
        );
        thread::sleep(Duration::from_secs(1));
    }

    fn run_http_api_wiring_phase(&self, plan: &RootExecutionPlan) -> Result<()> {
        match &plan.http_api.blocker {
            Some(detail) => self.runtime_adapter_blocker(RootPhase::HttpApiWiring, detail.clone()),
            None => Ok(()),
        }
    }

    fn run_sanity_check_phase(&self, plan: &RootExecutionPlan) -> Result<()> {
        if !self.config.rolling_restart {
            return Ok(());
        }

        let client = DockerCliAdapter::new();
        let containers = self.filtered_runtime_containers(&client, RootPhase::SanityCheck)?;
        self.check_runtime_sanity(plan, &containers)
    }

    fn emit_startup_phase(&self, plan: &RootExecutionPlan) {
        crate::startup::emit_startup_messages(&plan.startup_config);
    }

    fn run_once_branch(&self, _: &RootExecutionPlan) -> Result<()> {
        self.runtime_adapter_blocker(
            RootPhase::RunOnceUpdate,
            "legacy run_once needs runUpdatesWithNotifications, notifier close, and metric emission on top of a Docker-backed update executor".to_string(),
        )
    }

    fn http_api_only_branch(&self, plan: &RootExecutionPlan) -> Result<()> {
        self.run_multiple_instance_protection_phase(plan)?;
        self.apply_http_api_start_phase(plan)?;

        self.runtime_adapter_blocker(
            RootPhase::HttpApiStart,
            format!(
                "legacy HTTP API-only mode still needs a runtime adapter that can bind the server, share the update lock across {:?}, and execute update callbacks",
                plan.http_api.routes
            ),
        )
    }

    fn scheduled_branch(&self, plan: &RootExecutionPlan) -> Result<()> {
        self.run_multiple_instance_protection_phase(plan)?;
        self.apply_http_api_start_phase(plan)?;

        self.runtime_adapter_blocker(
            RootPhase::SchedulerLoop,
            format!(
                "legacy scheduler mode still needs a runtime adapter for periodic update execution, signal handling, and shutdown drain with shared wiring {:?}",
                plan.shared_runtime
            ),
        )
    }

    fn run_multiple_instance_protection_phase(&self, plan: &RootExecutionPlan) -> Result<()> {
        let client = DockerCliAdapter::new();
        let containers =
            self.filtered_watchtower_containers(&client, RootPhase::MultipleInstanceProtection)?;

        if self.watchtower_cleanup_plan(&containers).is_none() {
            return Ok(());
        }

        self.runtime_adapter_blocker(
            RootPhase::MultipleInstanceProtection,
            format!(
                "legacy CheckForMultipleWatchtowerInstances still needs cleanup execution for {:?} before {:?}",
                containers
                    .iter()
                    .map(|container| container.name().to_string())
                    .collect::<Vec<_>>(),
                plan.http_api.routes
            ),
        )
    }

    fn apply_http_api_start_phase(&self, plan: &RootExecutionPlan) -> Result<()> {
        match &plan.http_api.start_decision {
            None | Some(Ok(crate::api::StartDecision::Skipped)) => Ok(()),
            Some(Ok(crate::api::StartDecision::Start { .. })) => Ok(()),
            Some(Err(err)) => Err(Error::from(err.clone())),
        }
    }

    fn register_metrics_http_route(&self, api: &mut crate::api::Api) {
        let metrics_handler = crate::api_metrics::ApiMetrics::legacy();
        let (path, handle, metrics) = metrics_handler.into_parts();

        api.register_func(path, move |_| {
            crate::api::HttpResponse::plain(200, handle(&metrics))
        });
    }

    fn update_api_runtime_blocker_detail(&self) -> String {
        "legacy /v1/update wiring needs a shared root update executor adapter, such as `Fn(&[String]) -> Result<Option<crate::metrics::Metric>, String>`, so run_once, the scheduler, and the HTTP API all call the same Docker-backed update path with metrics emission".to_string()
    }

    fn runtime_adapter_blocker(&self, phase: RootPhase, detail: String) -> Result<()> {
        Err(self.runtime_adapter_error(phase, detail))
    }

    fn runtime_adapter_error(&self, phase: RootPhase, detail: String) -> Error {
        Error::RuntimeAdapterMissing { phase, detail }
    }

    fn filtered_runtime_containers<C>(&self, client: &C, phase: RootPhase) -> Result<Vec<Container>>
    where
        C: crate::lifecycle::LifecycleClient,
        C::Error: fmt::Display,
    {
        let (filter, _) = crate::filters::build_filter::<Container>(
            &self.config.containers,
            &self.config.disable_containers,
            self.config.label_enable,
            self.config.scope.as_deref().unwrap_or(""),
        );

        let mut containers = client.list_containers().map_err(|error| {
            self.runtime_adapter_error(
                phase,
                format!(
                    "legacy root orchestration needs a Docker runtime adapter that can list containers for filter `{}`: {error}",
                    self.filter_description()
                ),
            )
        })?;

        containers.retain(|container| filter(container));
        Ok(containers)
    }

    fn filtered_watchtower_containers<C>(
        &self,
        client: &C,
        phase: RootPhase,
    ) -> Result<Vec<Container>>
    where
        C: crate::lifecycle::LifecycleClient,
        C::Error: fmt::Display,
    {
        let mut filter: crate::filters::Filter<'_, Container> =
            Box::new(crate::filters::watchtower_only::<Container>);
        if let Some(scope) = self
            .config
            .scope
            .as_deref()
            .filter(|scope| !scope.is_empty())
        {
            filter = crate::filters::filter_by_scope(scope, filter);
        }

        let mut containers = client.list_containers().map_err(|error| {
            self.runtime_adapter_error(
                phase,
                format!(
                    "legacy CheckForMultipleWatchtowerInstances needs a Docker runtime adapter that can enumerate scoped watchtower containers before {:?}: {error}",
                    self.config.scope
                ),
            )
        })?;

        containers.retain(|container| filter(container));
        Ok(containers)
    }

    fn check_runtime_sanity(
        &self,
        plan: &RootExecutionPlan,
        containers: &[Container],
    ) -> Result<()> {
        actions::check_for_sanity(containers, self.config.rolling_restart).map_err(|error| {
            Error::InvalidConfig(format!(
                "{error} while checking containers for `{}`",
                plan.filter_description
            ))
        })
    }

    fn watchtower_cleanup_plan(
        &self,
        containers: &[Container],
    ) -> Option<actions::WatchtowerInstanceCleanupPlan> {
        actions::check_for_multiple_watchtower_instances(containers, self.config.cleanup)
    }

    fn filter_description(&self) -> String {
        crate::filters::build_filter_description(
            &self.config.containers,
            &self.config.disable_containers,
            self.config.label_enable,
            self.config.scope.as_deref().unwrap_or(""),
        )
    }
}

/// Convenience entrypoint for the binary crate.
pub fn run(config: impl Into<AppConfig>) -> Result<()> {
    WatchtowerApp::new(config).run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::container::{
        ContainerConfig, ContainerInspect, ContainerState, HostConfig, ImageInspect,
    };
    use crate::lifecycle::LifecycleClient;
    use crate::types::ImageID;

    #[derive(Clone)]
    struct MockLifecycleClient {
        containers: Vec<Container>,
    }

    impl MockLifecycleClient {
        fn new(containers: Vec<Container>) -> Self {
            Self { containers }
        }
    }

    impl LifecycleClient for MockLifecycleClient {
        type Error = String;

        fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error> {
            Ok(self.containers.clone())
        }

        fn get_container(
            &self,
            _container_id: &crate::types::ContainerID,
        ) -> std::result::Result<Container, Self::Error> {
            Err("not implemented".to_string())
        }

        fn execute_command(
            &self,
            _container_id: &crate::types::ContainerID,
            _command: &str,
            _timeout_minutes: i64,
        ) -> std::result::Result<bool, Self::Error> {
            Err("not implemented".to_string())
        }
    }

    fn base_config() -> AppConfig {
        AppConfig {
            interval: Some(Duration::from_secs(24 * 60 * 60)),
            timeout: Duration::from_secs(10),
            ..AppConfig::default()
        }
    }

    fn runtime_container(
        id: &str,
        name: &str,
        created: &str,
        labels: &[(&str, &str)],
        links: &[&str],
    ) -> Container {
        Container::new(
            ContainerInspect {
                id: crate::types::ContainerID::new(id),
                name: name.to_string(),
                image: ImageID::new("sha256:image"),
                created: created.to_string(),
                state: ContainerState {
                    running: true,
                    restarting: false,
                },
                config: Some(ContainerConfig {
                    image: "repo/image:latest".to_string(),
                    labels: labels
                        .iter()
                        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                        .collect::<BTreeMap<_, _>>(),
                    ..ContainerConfig::default()
                }),
                host_config: Some(HostConfig {
                    links: links.iter().map(|link| link.to_string()).collect(),
                    ..HostConfig::default()
                }),
                network_settings: None,
            },
            Some(ImageInspect {
                id: ImageID::new("sha256:image"),
                config: ContainerConfig::default(),
            }),
        )
    }

    #[test]
    fn health_check_exits_cleanly_for_non_pid_one_processes() {
        let app = WatchtowerApp::new(AppConfig {
            health_check: true,
            ..base_config()
        });

        assert_eq!(app.run_health_check_phase_with_pid(42), Ok(true));
    }

    #[test]
    fn health_check_blocks_pid_one_like_the_legacy_root() {
        let app = WatchtowerApp::new(AppConfig {
            health_check: true,
            ..base_config()
        });

        assert_eq!(
            app.run_health_check_phase_with_pid(1),
            Err(Error::HealthCheckOnMainProcess)
        );
    }

    #[test]
    fn run_once_branch_clears_periodic_startup_state() {
        let app = WatchtowerApp::new(AppConfig {
            run_once: true,
            ..base_config()
        });

        let plan = app.prepare_root_execution_plan();

        assert_eq!(plan.branch, RootBranch::RunOnce);
        assert_eq!(plan.startup_config.schedule, None);
        assert_eq!(plan.startup_config.interval, None);
        assert!(plan.startup_config.run_once);
    }

    #[test]
    fn blocking_http_api_branch_disables_periodic_startup_messages() {
        let app = WatchtowerApp::new(AppConfig {
            enable_http_update_api: true,
            http_api_token: Some("secret".to_string()),
            ..base_config()
        });

        let plan = app.prepare_root_execution_plan();

        assert_eq!(plan.branch, RootBranch::HttpApiOnly);
        assert_eq!(plan.startup_config.schedule, None);
        assert_eq!(plan.startup_config.interval, None);
        assert_eq!(plan.http_api.routes, vec![crate::api_update::PATH]);
        assert_eq!(plan.http_api.start_decision, None);
        assert_eq!(
            plan.http_api.blocker.as_deref(),
            Some(
                "legacy /v1/update wiring needs a shared root update executor adapter, such as `Fn(&[String]) -> Result<Option<crate::metrics::Metric>, String>`, so run_once, the scheduler, and the HTTP API all call the same Docker-backed update path with metrics emission"
            )
        );
    }

    #[test]
    fn sanity_check_skips_when_rolling_restart_is_disabled() {
        let app = WatchtowerApp::new(base_config());
        let plan = app.prepare_root_execution_plan();
        let containers = vec![runtime_container(
            "container-alpha",
            "alpha",
            "2024-06-18T12:00:00Z",
            &[],
            &["/beta"],
        )];

        assert_eq!(app.check_runtime_sanity(&plan, &containers), Ok(()));
    }

    #[test]
    fn sanity_check_rejects_linked_containers_during_rolling_restart() {
        let app = WatchtowerApp::new(AppConfig {
            rolling_restart: true,
            ..base_config()
        });
        let plan = app.prepare_root_execution_plan();
        let containers = vec![runtime_container(
            "container-alpha",
            "alpha",
            "2024-06-18T12:00:00Z",
            &[],
            &["/beta"],
        )];

        assert_eq!(
            app.check_runtime_sanity(&plan, &containers),
            Err(Error::InvalidConfig(
                "\"alpha\" is depending on at least one other container. This is not compatible with rolling restarts while checking containers for `Checking all containers (except explicitly disabled with label)`".to_string()
            ))
        );
    }

    #[test]
    fn filtered_runtime_containers_apply_the_root_selection_rules() {
        let app = WatchtowerApp::new(AppConfig {
            containers: vec!["alpha".to_string()],
            scope: Some("prod".to_string()),
            ..base_config()
        });
        let client = MockLifecycleClient::new(vec![
            runtime_container(
                "container-alpha",
                "alpha",
                "2024-06-18T12:00:00Z",
                &[("com.centurylinklabs.watchtower.scope", "prod")],
                &[],
            ),
            runtime_container(
                "container-beta",
                "beta",
                "2024-06-18T12:00:00Z",
                &[("com.centurylinklabs.watchtower.scope", "dev")],
                &[],
            ),
        ]);

        let filtered = app
            .filtered_runtime_containers(&client, RootPhase::SanityCheck)
            .expect("filtering should succeed");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name(), "alpha");
    }

    #[test]
    fn filtered_watchtower_containers_respect_scope_and_cleanup_plan() {
        let app = WatchtowerApp::new(AppConfig {
            scope: Some("prod".to_string()),
            cleanup: true,
            ..base_config()
        });
        let client = MockLifecycleClient::new(vec![
            runtime_container(
                "watchtower-old",
                "watchtower-old",
                "2024-06-18T12:00:00Z",
                &[
                    ("com.centurylinklabs.watchtower", "true"),
                    ("com.centurylinklabs.watchtower.scope", "prod"),
                ],
                &[],
            ),
            runtime_container(
                "watchtower-new",
                "watchtower-new",
                "2024-06-19T12:00:00Z",
                &[
                    ("com.centurylinklabs.watchtower", "true"),
                    ("com.centurylinklabs.watchtower.scope", "prod"),
                ],
                &[],
            ),
            runtime_container(
                "watchtower-other",
                "watchtower-other",
                "2024-06-19T13:00:00Z",
                &[
                    ("com.centurylinklabs.watchtower", "true"),
                    ("com.centurylinklabs.watchtower.scope", "dev"),
                ],
                &[],
            ),
        ]);

        let filtered = app
            .filtered_watchtower_containers(&client, RootPhase::MultipleInstanceProtection)
            .expect("filtering should succeed");
        let plan = app.watchtower_cleanup_plan(&filtered);

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name(), "watchtower-old");
        assert_eq!(filtered[1].name(), "watchtower-new");
        assert!(plan.is_some());
        assert_eq!(
            plan.expect("cleanup plan should exist").stop_container_ids,
            vec![crate::types::ContainerID::new("watchtower-old")]
        );
    }

    #[test]
    fn http_update_api_surfaces_the_root_wiring_blocker_before_runtime_phases() {
        let app = WatchtowerApp::new(AppConfig {
            enable_http_update_api: true,
            http_api_token: Some("secret".to_string()),
            ..base_config()
        });
        let plan = app.prepare_root_execution_plan();

        assert_eq!(
            app.run_http_api_wiring_phase(&plan),
            Err(Error::RuntimeAdapterMissing {
                phase: RootPhase::HttpApiWiring,
                detail: "legacy /v1/update wiring needs a shared root update executor adapter, such as `Fn(&[String]) -> Result<Option<crate::metrics::Metric>, String>`, so run_once, the scheduler, and the HTTP API all call the same Docker-backed update path with metrics emission".to_string(),
            })
        );
    }

    #[test]
    fn metrics_http_api_uses_the_real_api_start_decision() {
        let app = WatchtowerApp::new(AppConfig {
            enable_http_metrics_api: true,
            http_api_token: Some("secret".to_string()),
            ..base_config()
        });

        let plan = app.prepare_root_execution_plan();

        assert_eq!(plan.branch, RootBranch::Scheduled);
        assert_eq!(plan.http_api.routes, vec![crate::api_metrics::PATH]);
        assert_eq!(plan.http_api.blocker, None);
        assert_eq!(
            plan.http_api.start_decision,
            Some(Ok(crate::api::StartDecision::Start { block: false }))
        );
    }

    #[test]
    fn scheduled_branch_marks_update_api_as_blocked_until_runtime_adapter_exists() {
        let app = WatchtowerApp::new(AppConfig {
            enable_http_update_api: true,
            unblock_http_api: true,
            http_api_token: Some("secret".to_string()),
            interval: Some(Duration::from_secs(300)),
            ..base_config()
        });

        let plan = app.prepare_root_execution_plan();

        assert_eq!(plan.branch, RootBranch::Scheduled);
        assert_eq!(plan.startup_config.interval, Some(Duration::from_secs(300)));
        assert_eq!(plan.http_api.routes, vec![crate::api_update::PATH]);
        assert_eq!(plan.http_api.start_decision, None);
        assert_eq!(
            plan.http_api.blocker.as_deref(),
            Some(
                "legacy /v1/update wiring needs a shared root update executor adapter, such as `Fn(&[String]) -> Result<Option<crate::metrics::Metric>, String>`, so run_once, the scheduler, and the HTTP API all call the same Docker-backed update path with metrics emission"
            )
        );
        assert!(
            plan.shared_runtime
                .update_lock_shared_between_scheduler_and_api
        );
        assert!(plan.shared_runtime.shutdown_waits_for_running_update);
    }

    #[test]
    fn watchtower_cleanup_plan_is_none_for_a_single_instance() {
        let app = WatchtowerApp::new(AppConfig {
            scope: Some("prod".to_string()),
            cleanup: true,
            ..base_config()
        });
        let containers = vec![runtime_container(
            "watchtower-single",
            "watchtower-single",
            "2024-06-20T12:00:00Z",
            &[
                ("com.centurylinklabs.watchtower", "true"),
                ("com.centurylinklabs.watchtower.scope", "prod"),
            ],
            &[],
        )];

        assert!(app.watchtower_cleanup_plan(&containers).is_none());
    }
}
