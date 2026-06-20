#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Docker client helper snapshots translated from the legacy Go client layer.
//!
//! SoT mapping: `IDX-0070` maps `old-source/pkg/container/client.go` to this file.
//!
//! The actual Docker HTTP transport is not implemented here. This module keeps
//! the deterministic parts that can be exercised without a live daemon:
//! warning strategy selection and network alias normalization.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::actions::UpdateClient;
use crate::container::{
    Container, ContainerConfig, ContainerInspect, ContainerState, HealthConfig, HostConfig,
    ImageInspect, NetworkMode, PortBinding,
};
use crate::lifecycle::LifecycleClient;
use crate::registry::{digest, pull, trust};
use crate::types::{ContainerID, FilterableContainer, ImageID, UpdateParams};

const DOCKER_BINARY: &str = "docker";
const EXEC_SKIP_UPDATE_EXIT_CODE: i32 = 75;
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_STOP_SIGNAL: &str = "SIGTERM";

/// Client configuration mirrored from the legacy Go wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientOptions {
    pub remove_volumes: bool,
    pub include_stopped: bool,
    pub revive_stopped: bool,
    pub include_restarting: bool,
    pub warn_on_head_failed: WarningStrategy,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            remove_volumes: false,
            include_stopped: false,
            revive_stopped: false,
            include_restarting: false,
            warn_on_head_failed: WarningStrategy::default(),
        }
    }
}

/// Docker CLI transport errors.
#[derive(Debug, Error)]
pub enum DockerCliError {
    #[error("failed to spawn `{program}` for {context}: {source}")]
    Spawn {
        program: String,
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("docker command timed out while {context} after {timeout:?}")]
    Timeout { context: String, timeout: Duration },
    #[error("docker command failed while {context} with status {status:?}: {stderr}")]
    CommandFailed {
        context: String,
        status: Option<i32>,
        stdout: String,
        stderr: String,
    },
    #[error("failed to parse docker json for {context}: {source}")]
    Json {
        context: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("docker inspect returned no {kind} for `{target}`")]
    MissingInspect { kind: &'static str, target: String },
    #[error("unsupported docker recreate config for `{container}`: {detail}")]
    UnsupportedConfig { container: String, detail: String },
    #[error("invalid container config: {0}")]
    InvalidConfig(String),
    #[error("registry helper failed: {0}")]
    Registry(String),
}

/// Small Docker adapter backed by the `docker` CLI.
///
/// This is intentionally fail-closed. When the inspected runtime config cannot
/// be reproduced safely with the supported CLI translation, the adapter returns
/// an explicit error instead of silently dropping fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerCliAdapter {
    docker_bin: String,
    options: ClientOptions,
}

impl Default for DockerCliAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerCliAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            docker_bin: DOCKER_BINARY.to_string(),
            options: ClientOptions::default(),
        }
    }

    #[must_use]
    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            docker_bin: binary.into(),
            options: ClientOptions::default(),
        }
    }

    #[must_use]
    pub fn with_options(binary: impl Into<String>, options: ClientOptions) -> Self {
        Self {
            docker_bin: binary.into(),
            options,
        }
    }

    #[must_use]
    pub fn options(&self) -> &ClientOptions {
        &self.options
    }

    #[must_use]
    pub fn warn_on_head_pull_failed(&self, container: &Container) -> bool {
        warn_on_head_pull_failed(self.options.warn_on_head_failed, container.image_name())
    }

    fn inspect_container_model(&self, container_id: &ContainerID) -> Result<Container, DockerCliError> {
        let mut inspect = self.inspect_container(container_id.as_str())?;
        if let Some(parent_id) = inspect.network_container_id().map(str::to_string) {
            if let Ok(parent) = self.inspect_container(parent_id.as_str()) {
                inspect.rewrite_network_mode_with_name(parent.name.as_str());
            }
        }

        let image_info = self.inspect_image_model(inspect.image.as_str()).ok();
        let image_info = image_info.map(CliImageInspect::into_image_inspect);
        Ok(Container::new(inspect.into_container_inspect(), image_info))
    }

    fn list_container_models(&self) -> Result<Vec<Container>, DockerCliError> {
        let list_filter = self.create_list_filter();
        let mut args = vec![
            "ps".to_string(),
            "--no-trunc".to_string(),
            "--quiet".to_string(),
        ];
        if list_filter.include_all {
            args.push("--all".to_string());
        }
        for status in list_filter.statuses {
            args.push("--filter".to_string());
            args.push(format!("status={status}"));
        }

        let ids = self
            .stdout_trimmed(args, "listing containers")?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if ids.is_empty() {
            return Ok(Vec::new());
        }

        ids.into_iter()
            .map(|id| self.inspect_container_model(&ContainerID::new(id)))
            .collect()
    }

    fn create_list_filter(&self) -> ListFilter {
        let mut statuses = vec!["running"];
        let include_all = self.options.include_stopped || self.options.include_restarting;

        if self.options.include_stopped {
            statuses.push("created");
            statuses.push("exited");
        }

        if self.options.include_restarting {
            statuses.push("restarting");
        }

        ListFilter {
            include_all,
            statuses,
        }
    }

    fn inspect_containers(&self, ids: &[String]) -> Result<Vec<CliContainerInspect>, DockerCliError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let args = ids
            .iter()
            .cloned()
            .fold(vec!["inspect".to_string()], |mut args, id| {
                args.push(id);
                args
            });
        self.json(args, "inspecting containers")
    }

    fn inspect_container(&self, id: &str) -> Result<CliContainerInspect, DockerCliError> {
        let context = format!("inspecting container `{id}`");
        let output = self.run(["inspect", id], None, context.clone())?;
        if !output.status.success() {
            if is_missing_container_error(output.stderr.as_str()) {
                return Err(DockerCliError::MissingInspect {
                    kind: "container",
                    target: id.to_string(),
                });
            }
            return Err(DockerCliError::CommandFailed {
                context: output.context,
                status: output.status.code(),
                stdout: output.stdout,
                stderr: output.stderr,
            });
        }

        let mut items: Vec<CliContainerInspect> =
            serde_json::from_str(output.stdout.trim()).map_err(|source| DockerCliError::Json {
                context,
                source,
            })?;
        items.pop().ok_or_else(|| DockerCliError::MissingInspect {
            kind: "container",
            target: id.to_string(),
        })
    }

    fn inspect_images_by_id(
        &self,
        image_ids: &[String],
    ) -> Result<HashMap<String, CliImageInspect>, DockerCliError> {
        if image_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let args = image_ids
            .iter()
            .cloned()
            .fold(vec!["image".to_string(), "inspect".to_string()], |mut args, id| {
                args.push(id);
                args
            });
        let images: Vec<CliImageInspect> = self.json(args, "inspecting images")?;
        Ok(images
            .into_iter()
            .map(|image| (image.id.clone(), image))
            .collect())
    }

    fn inspect_image_model(&self, image_ref: &str) -> Result<CliImageInspect, DockerCliError> {
        let items: Vec<CliImageInspect> =
            self.json(["image", "inspect", image_ref], format!("inspecting image `{image_ref}`"))?;
        items
            .into_iter()
            .next()
            .ok_or_else(|| DockerCliError::MissingInspect {
                kind: "image",
                target: image_ref.to_string(),
            })
    }

    fn inspect_image(&self, image_ref: &str) -> Result<ImageInspect, DockerCliError> {
        self.inspect_image_model(image_ref)
            .map(CliImageInspect::into_image_inspect)
    }

    fn pull_image(&self, container: &Container) -> Result<(), DockerCliError> {
        let image_name = container.image_name();
        if image_name.starts_with("sha256:") {
            return Err(DockerCliError::InvalidConfig(
                "container uses a pinned image, and cannot be updated by watchtower".to_string(),
            ));
        }

        let options = pull::get_pull_options(image_name)
            .map_err(|err| DockerCliError::Registry(err.to_string()))?;
        let repo_digests = self
            .inspect_image_model(image_name)
            .ok()
            .map(|image| image.repo_digests)
            .unwrap_or_default();
        let pull_decision = pull::decide_pull_action(
            digest::compare_digest(image_name, &repo_digests, &options.registry_auth),
            self.warn_on_head_pull_failed(container),
        );

        if matches!(pull_decision, pull::PullDecision::SkipPull) {
            return Ok(());
        }

        self.success(["pull", image_name], format!("pulling image `{image_name}`"))
    }

    fn recreate_container(&self, container: &Container) -> Result<ContainerID, DockerCliError> {
        let mut container = container.clone();
        let config = container
            .get_create_config()
            .map_err(|err| DockerCliError::InvalidConfig(err.to_string()))?;
        let host_config = container
            .get_create_host_config()
            .map_err(|err| DockerCliError::InvalidConfig(err.to_string()))?;
        let network_config = container.get_network_config();
        let simple_network = simple_network_config(&network_config);

        let created_id = self.stdout_trimmed(
            create_command_args(&container, &config, &host_config, &simple_network),
            format!("creating replacement for `{}`", container.name()),
        )?;
        let created_id = created_id.trim().to_string();

        if created_id.is_empty() {
            return Err(DockerCliError::MissingInspect {
                kind: "created container id",
                target: container.name().to_string(),
            });
        }

        if !host_config.network_mode.is_host() {
            for network_name in simple_network.endpoints.keys() {
                self.success(
                    [
                        "network",
                        "disconnect",
                        network_name.as_str(),
                        created_id.as_str(),
                        "--force",
                    ],
                    format!(
                        "disconnecting recreated container `{}` from network `{network_name}`",
                        container.name()
                    ),
                )?;
            }

            let mut network_names = network_config.endpoints.keys().cloned().collect::<Vec<_>>();
            network_names.sort();
            for network_name in network_names {
                let endpoint = network_config
                    .endpoints
                    .get(network_name.as_str())
                    .cloned()
                    .unwrap_or_default();

                let mut args = vec!["network".to_string(), "connect".to_string()];
                for alias in endpoint.aliases {
                    args.push("--alias".to_string());
                    args.push(alias);
                }
                args.push(network_name.clone());
                args.push(created_id.clone());

                self.success(
                    args,
                    format!(
                        "connecting recreated container `{}` to network `{network_name}`",
                        container.name()
                    ),
                )?;
            }
        }

        let created_id = ContainerID::new(created_id);
        if !container.is_running() && !self.options.revive_stopped {
            return Ok(created_id);
        }

        self.success(
            ["start", created_id.as_str()],
            format!("starting recreated container `{}`", container.name()),
        )?;

        Ok(created_id)
    }

    fn wait_for_stop_or_timeout(
        &self,
        container: &Container,
        wait_time: Duration,
    ) -> Result<(), DockerCliError> {
        let deadline = Instant::now() + wait_time;
        loop {
            if Instant::now() >= deadline {
                return Ok(());
            }

            let inspected = self.inspect_container(container.id().as_str())?;
            if !inspected.state.running {
                return Ok(());
            }

            thread::sleep(Duration::from_secs(1));
        }
    }

    fn wait_for_removal_or_timeout(
        &self,
        id: &str,
        wait_time: Duration,
    ) -> Result<(), DockerCliError> {
        let deadline = Instant::now() + wait_time;
        loop {
            if Instant::now() >= deadline {
                return Err(DockerCliError::Timeout {
                    context: format!("waiting for container removal: {}", ContainerID::new(id).short_id()),
                    timeout: wait_time,
                });
            }

            match self.inspect_container(id) {
                Ok(_) => thread::sleep(Duration::from_secs(1)),
                Err(DockerCliError::MissingInspect { .. }) => return Ok(()),
                Err(err) => return Err(err),
            }
        }
    }

    fn stdout_trimmed<I, S>(&self, args: I, context: impl Into<String>) -> Result<String, DockerCliError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let output = self.run(args, None, context)?;
        self.ensure_success(output).map(|output| output.stdout)
    }

    fn success<I, S>(&self, args: I, context: impl Into<String>) -> Result<(), DockerCliError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let output = self.run(args, None, context)?;
        self.ensure_success(output)?;
        Ok(())
    }

    fn json<T, I, S>(&self, args: I, context: impl Into<String>) -> Result<T, DockerCliError>
    where
        T: for<'de> Deserialize<'de>,
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let context = context.into();
        let output = self.run(args, None, context.clone())?;
        let output = self.ensure_success(output)?;
        serde_json::from_str(output.stdout.trim()).map_err(|source| DockerCliError::Json {
            context,
            source,
        })
    }

    fn run<I, S>(
        &self,
        args: I,
        timeout: Option<Duration>,
        context: impl Into<String>,
    ) -> Result<CommandOutput, DockerCliError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let context = context.into();
        let args = args
            .into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect::<Vec<_>>();
        let timeout = timeout.unwrap_or(Duration::from_secs(0));

        let mut child = Command::new(&self.docker_bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| DockerCliError::Spawn {
                program: self.docker_bin.clone(),
                context: context.clone(),
                source,
            })?;

        if timeout.is_zero() {
            let output = child
                .wait_with_output()
                .map_err(|source| DockerCliError::Spawn {
                    program: self.docker_bin.clone(),
                    context: context.clone(),
                    source,
                })?;
            return Ok(CommandOutput::new(
                context,
                output.status,
                output.stdout,
                output.stderr,
            ));
        }

        let started_at = Instant::now();
        loop {
            if child
                .try_wait()
                .map_err(|source| DockerCliError::Spawn {
                    program: self.docker_bin.clone(),
                    context: context.clone(),
                    source,
                })?
                .is_some()
            {
                let output = child
                    .wait_with_output()
                    .map_err(|source| DockerCliError::Spawn {
                        program: self.docker_bin.clone(),
                        context: context.clone(),
                        source,
                    })?;
                return Ok(CommandOutput::new(
                    context,
                    output.status,
                    output.stdout,
                    output.stderr,
                ));
            }

            if started_at.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return Err(DockerCliError::Timeout { context, timeout });
            }

            thread::sleep(DEFAULT_POLL_INTERVAL);
        }
    }

    fn ensure_success(&self, output: CommandOutput) -> Result<CommandOutput, DockerCliError> {
        if output.status.success() {
            Ok(output)
        } else {
            Err(DockerCliError::CommandFailed {
                context: output.context,
                status: output.status.code(),
                stdout: output.stdout,
                stderr: output.stderr,
            })
        }
    }
}

impl LifecycleClient for DockerCliAdapter {
    type Error = DockerCliError;

    fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error> {
        self.list_container_models()
    }

    fn get_container(
        &self,
        container_id: &ContainerID,
    ) -> std::result::Result<Container, Self::Error> {
        self.inspect_container_model(container_id)
    }

    fn execute_command(
        &self,
        container_id: &ContainerID,
        command: &str,
        timeout_minutes: i64,
    ) -> std::result::Result<bool, Self::Error> {
        let timeout_seconds = timeout_minutes.max(0) as u64 * 60;
        let output = self.run(
            ["exec", container_id.as_str(), "sh", "-c", command],
            Some(Duration::from_secs(timeout_seconds)),
            format!("executing lifecycle command in `{container_id}`"),
        )?;

        match output.status.code() {
            Some(0) => Ok(false),
            Some(EXEC_SKIP_UPDATE_EXIT_CODE) => Ok(true),
            _ => Err(DockerCliError::CommandFailed {
                context: output.context,
                status: output.status.code(),
                stdout: output.stdout,
                stderr: output.stderr,
            }),
        }
    }
}

impl UpdateClient for DockerCliAdapter {
    fn is_container_stale(
        &self,
        container: &Container,
        params: &UpdateParams,
    ) -> std::result::Result<(bool, ImageID), Self::Error> {
        if !container.is_no_pull(params) {
            self.pull_image(container)?;
        }

        let current_image = container
            .container_info()
            .map(|info| info.image.clone())
            .unwrap_or_else(|| container.image_id().clone());
        let pulled = self.inspect_image(container.image_name())?;
        let new_image = pulled.id.clone();
        Ok((new_image != current_image, new_image))
    }

    fn stop_container(
        &self,
        container: &Container,
        timeout: Duration,
    ) -> std::result::Result<(), Self::Error> {
        let id_str = container.id().as_str().to_string();
        let short_id = container.id().short_id();
        let signal = {
            let stop_signal = container.stop_signal();
            if stop_signal.is_empty() {
                DEFAULT_STOP_SIGNAL.to_string()
            } else {
                stop_signal
            }
        };

        if container.is_running() {
            self.success(
                ["kill", "--signal", signal.as_str(), id_str.as_str()],
                format!("stopping container `{}`", container.id()),
            )?;
        }

        self.wait_for_stop_or_timeout(container, timeout)?;

        let auto_remove = container
            .container_info()
            .and_then(|info| info.host_config.as_ref())
            .is_some_and(|host_config| host_config.auto_remove);
        if !auto_remove {
            let mut args = vec!["rm".to_string(), "--force".to_string()];
            if self.options.remove_volumes {
                args.push("--volumes".to_string());
            }
            args.push(id_str.clone());
            match self.success(args, format!("removing container `{short_id}`")) {
                Ok(()) => {}
                Err(DockerCliError::CommandFailed { stderr, .. })
                    if is_missing_container_error(stderr.as_str()) =>
                {
                    return Ok(());
                }
                Err(err) => return Err(err),
            }
        }

        self.wait_for_removal_or_timeout(id_str.as_str(), timeout)
    }

    fn start_container(
        &self,
        container: &Container,
    ) -> std::result::Result<ContainerID, Self::Error> {
        self.recreate_container(container)
    }

    fn rename_container(
        &self,
        container: &Container,
        new_name: &str,
    ) -> std::result::Result<(), Self::Error> {
        self.success(
            ["rename", container.id().as_str(), new_name],
            format!("renaming container `{}` to `{new_name}`", container.id()),
        )
    }

    fn remove_image_by_id(&self, image_id: &ImageID) -> std::result::Result<(), Self::Error> {
        self.success(
            ["image", "rm", image_id.as_str()],
            format!("removing image `{image_id}`"),
        )
    }
}

#[derive(Debug)]
struct CommandOutput {
    context: String,
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ListFilter {
    include_all: bool,
    statuses: Vec<&'static str>,
}

impl CommandOutput {
    fn new(context: String, status: ExitStatus, stdout: Vec<u8>, stderr: Vec<u8>) -> Self {
        Self {
            context,
            status,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliContainerInspect {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    created: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    state: CliContainerState,
    config: Option<CliConfig>,
    host_config: Option<CliHostConfig>,
    network_settings: Option<CliNetworkSettings>,
    #[serde(default)]
    mounts: Vec<CliMount>,
}

impl CliContainerInspect {
    fn network_container_id(&self) -> Option<&str> {
        self.host_config
            .as_ref()
            .and_then(|host_config| host_config.network_mode.strip_prefix("container:"))
            .filter(|value| !value.is_empty())
    }

    fn rewrite_network_mode_with_name(&mut self, parent_name: &str) {
        if let Some(host_config) = self.host_config.as_mut() {
            host_config.network_mode = format!("container:{parent_name}");
        }
    }

    fn into_container_inspect(self) -> ContainerInspect {
        ContainerInspect {
            id: ContainerID::new(self.id),
            name: self.name,
            image: ImageID::new(self.image),
            created: self.created,
            state: ContainerState {
                running: self.state.running,
                restarting: self.state.restarting,
            },
            config: self.config.as_ref().map(CliConfig::to_container_config),
            host_config: self.host_config.as_ref().map(CliHostConfig::to_host_config),
            network_settings: self
                .network_settings
                .and_then(|settings| settings.into_network_settings()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliImageInspect {
    #[serde(default)]
    id: String,
    config: Option<CliConfig>,
    #[serde(default)]
    repo_digests: Vec<String>,
}

impl CliImageInspect {
    fn into_image_inspect(self) -> ImageInspect {
        ImageInspect {
            id: ImageID::new(self.id),
            config: self
                .config
                .as_ref()
                .map(CliConfig::to_container_config)
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct CliContainerState {
    #[serde(default)]
    running: bool,
    #[serde(default)]
    restarting: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliConfig {
    #[serde(default)]
    image: String,
    labels: Option<BTreeMap<String, String>>,
    #[serde(default)]
    working_dir: String,
    #[serde(default)]
    user: String,
    entrypoint: Option<Vec<String>>,
    cmd: Option<Vec<String>>,
    env: Option<Vec<String>>,
    volumes: Option<BTreeMap<String, Value>>,
    exposed_ports: Option<BTreeMap<String, Value>>,
    healthcheck: Option<CliHealthcheck>,
    #[serde(default)]
    hostname: String,
    #[serde(default)]
    domainname: String,
    #[serde(default)]
    open_stdin: bool,
    #[serde(default)]
    tty: bool,
    stop_signal: Option<String>,
}

impl CliConfig {
    fn to_container_config(&self) -> ContainerConfig {
        ContainerConfig {
            image: self.image.clone(),
            labels: self.labels.clone().unwrap_or_default(),
            working_dir: self.working_dir.clone(),
            user: self.user.clone(),
            entrypoint: self.entrypoint.clone().unwrap_or_default(),
            cmd: self.cmd.clone().unwrap_or_default(),
            env: self.env.clone().unwrap_or_default(),
            volumes: self
                .volumes
                .as_ref()
                .map(|volumes| volumes.keys().cloned().collect::<BTreeSet<_>>())
                .unwrap_or_default(),
            exposed_ports: self
                .exposed_ports
                .as_ref()
                .map(|ports| ports.keys().cloned().collect::<BTreeSet<_>>()),
            healthcheck: self.healthcheck.as_ref().map(CliHealthcheck::to_health_config),
            hostname: self.hostname.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliHealthcheck {
    #[serde(default)]
    test: Vec<String>,
    #[serde(default)]
    interval: i64,
    #[serde(default)]
    timeout: i64,
    #[serde(default)]
    start_period: i64,
    #[serde(default)]
    retries: u32,
}

impl CliHealthcheck {
    fn to_health_config(&self) -> HealthConfig {
        HealthConfig {
            test: self.test.clone(),
            interval: duration_from_nanos(self.interval),
            timeout: duration_from_nanos(self.timeout),
            start_period: duration_from_nanos(self.start_period),
            retries: self.retries,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliHostConfig {
    links: Option<Vec<String>>,
    #[serde(default)]
    network_mode: String,
    port_bindings: Option<BTreeMap<String, Vec<CliPortBinding>>>,
    #[serde(default)]
    auto_remove: bool,
    restart_policy: Option<CliRestartPolicy>,
    log_config: Option<CliLogConfig>,
    #[serde(default)]
    privileged: bool,
    #[serde(default)]
    readonly_rootfs: bool,
    cap_add: Option<Vec<String>>,
    cap_drop: Option<Vec<String>>,
    dns: Option<Vec<String>>,
    dns_options: Option<Vec<String>>,
    dns_search: Option<Vec<String>>,
    extra_hosts: Option<Vec<String>>,
    group_add: Option<Vec<String>>,
    #[serde(default)]
    ipc_mode: String,
    #[serde(default)]
    pid_mode: String,
    security_opt: Option<Vec<String>>,
    tmpfs: Option<BTreeMap<String, String>>,
    ulimits: Option<Vec<CliUlimit>>,
    #[serde(default)]
    userns_mode: String,
    #[serde(default)]
    shm_size: i64,
    volumes_from: Option<Vec<String>>,
    #[serde(default)]
    publish_all_ports: bool,
    #[serde(default)]
    memory: i64,
    #[serde(default)]
    nano_cpus: i64,
    #[serde(default)]
    cpu_shares: i64,
    #[serde(default)]
    cpuset_cpus: String,
    #[serde(default)]
    cpuset_mems: String,
    devices: Option<Vec<Value>>,
    device_cgroup_rules: Option<Vec<String>>,
    device_requests: Option<Vec<Value>>,
}

impl CliHostConfig {
    fn to_host_config(&self) -> HostConfig {
        HostConfig {
            links: self.links.clone().unwrap_or_default(),
            network_mode: parse_network_mode(&self.network_mode),
            port_bindings: self
                .port_bindings
                .as_ref()
                .map(|bindings| {
                    bindings
                        .iter()
                        .map(|(port, values)| {
                            (
                                port.clone(),
                                values
                                    .iter()
                                    .map(|binding| PortBinding {
                                        host_ip: empty_to_none(binding.host_ip.as_deref()),
                                        host_port: empty_to_none(binding.host_port.as_deref()),
                                    })
                                    .collect::<Vec<_>>(),
                            )
                        })
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default(),
            auto_remove: self.auto_remove,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliPortBinding {
    host_ip: Option<String>,
    host_port: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct CliRestartPolicy {
    #[serde(default)]
    name: String,
    #[serde(default)]
    maximum_retry_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliLogConfig {
    #[serde(default)]
    r#type: String,
    config: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct CliUlimit {
    #[serde(default)]
    name: String,
    #[serde(default)]
    soft: i64,
    #[serde(default)]
    hard: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct CliNetworkSettings {
    networks: Option<HashMap<String, CliNetworkEndpoint>>,
}

impl CliNetworkSettings {
    fn into_network_settings(self) -> Option<HashMap<String, NetworkEndpoint>> {
        self.networks.map(|networks| {
            networks
                .into_iter()
                .map(|(name, endpoint)| {
                    (
                        name,
                        NetworkEndpoint {
                            aliases: endpoint.aliases.unwrap_or_default(),
                        },
                    )
                })
                .collect()
        })
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct CliNetworkEndpoint {
    aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct CliMount {
    #[serde(default)]
    r#type: String,
    name: Option<String>,
    #[serde(default)]
    source: String,
    #[serde(default)]
    destination: String,
    #[serde(rename = "RW", default)]
    rw: bool,
    #[serde(default)]
    propagation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatePlan {
    args: Vec<String>,
    extra_networks: Vec<NetworkConnectPlan>,
}

impl CreatePlan {
    fn from_inspect(desired_name: &str, inspect: &CliContainerInspect) -> Result<Self, DockerCliError> {
        let container_name = trim_container_name(desired_name);
        let config = inspect
            .config
            .as_ref()
            .ok_or_else(|| DockerCliError::UnsupportedConfig {
                container: container_name.to_string(),
                detail: "container inspect did not include `Config`".to_string(),
            })?;
        let host_config = inspect
            .host_config
            .as_ref()
            .ok_or_else(|| DockerCliError::UnsupportedConfig {
                container: container_name.to_string(),
                detail: "container inspect did not include `HostConfig`".to_string(),
            })?;

        validate_supported_recreate(container_name, host_config)?;

        let mut args = vec![
            "container".to_string(),
            "create".to_string(),
            "--name".to_string(),
            container_name.to_string(),
        ];

        append_basic_create_args(&mut args, config, host_config);
        append_mount_args(&mut args, container_name, &inspect.mounts)?;

        let primary_network = determine_primary_network(host_config, inspect);
        if let Some(primary) = &primary_network {
            args.push("--network".to_string());
            args.push(primary.name.clone());
            for alias in &primary.aliases {
                args.push("--network-alias".to_string());
                args.push(alias.clone());
            }
        }

        append_entrypoint_option(&mut args, config);
        args.push(config.image.clone());
        append_command_tail(&mut args, config);

        let extra_networks = inspect
            .network_settings
            .as_ref()
            .map(|settings| {
                normalized_network_endpoints(settings, inspect.id.as_str())
                    .into_iter()
                    .filter(|connection| {
                        primary_network
                            .as_ref()
                            .is_none_or(|primary| primary.name != connection.name)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(Self { args, extra_networks })
    }

    fn create_args(&self) -> Vec<&str> {
        self.args.iter().map(String::as_str).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NetworkConnectPlan {
    name: String,
    aliases: Vec<String>,
}

impl NetworkConnectPlan {
    fn connect_options(&self) -> Vec<String> {
        let mut args = Vec::new();
        for alias in &self.aliases {
            args.push("--alias".to_string());
            args.push(alias.clone());
        }
        args
    }
}

fn is_missing_container_error(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("no such container") || stderr.contains("no such object")
}

fn create_command_args(
    container: &Container,
    config: &ContainerConfig,
    host_config: &HostConfig,
    network_config: &NetworkingConfig,
) -> Vec<String> {
    let mut args = vec![
        "container".to_string(),
        "create".to_string(),
        "--name".to_string(),
        trim_container_name(container.name()).to_string(),
    ];

    append_create_config_args(&mut args, container, config);
    append_host_config_args(&mut args, host_config);

    if let Some((network_name, endpoint)) = network_config.endpoints.iter().next() {
        args.push("--network".to_string());
        args.push(network_name.clone());
        for alias in &endpoint.aliases {
            args.push("--network-alias".to_string());
            args.push(alias.clone());
        }
    }

    append_entrypoint_args(&mut args, config);
    args.push(config.image.clone());
    append_command_args(&mut args, config);

    args
}

fn append_create_config_args(args: &mut Vec<String>, container: &Container, config: &ContainerConfig) {
    if !config.hostname.is_empty() {
        args.push("--hostname".to_string());
        args.push(config.hostname.clone());
    }
    if !config.user.is_empty() {
        args.push("--user".to_string());
        args.push(config.user.clone());
    }
    if !config.working_dir.is_empty() {
        args.push("--workdir".to_string());
        args.push(config.working_dir.clone());
    }

    for env in &config.env {
        args.push("--env".to_string());
        args.push(env.clone());
    }
    for (key, value) in &config.labels {
        args.push("--label".to_string());
        args.push(format!("{key}={value}"));
    }
    for volume in &config.volumes {
        args.push("--volume".to_string());
        args.push(volume.clone());
    }
    for port in config.exposed_ports.as_ref().into_iter().flatten() {
        args.push("--expose".to_string());
        args.push(port.clone());
    }
    if let Some(healthcheck) = config.healthcheck.as_ref() {
        append_healthcheck_runtime_args(args, healthcheck);
    }

    let stop_signal = container.stop_signal();
    if !stop_signal.is_empty() {
        args.push("--stop-signal".to_string());
        args.push(stop_signal);
    }
}

fn append_host_config_args(args: &mut Vec<String>, host_config: &HostConfig) {
    for link in &host_config.links {
        args.push("--link".to_string());
        args.push(link.clone());
    }

    match &host_config.network_mode {
        NetworkMode::Default => {}
        NetworkMode::Host => {
            args.push("--network".to_string());
            args.push("host".to_string());
        }
        NetworkMode::Container(name) => {
            args.push("--network".to_string());
            args.push(format!("container:{name}"));
        }
        NetworkMode::Other(mode) => {
            args.push("--network".to_string());
            args.push(mode.clone());
        }
    }

    for (port, bindings) in &host_config.port_bindings {
        if bindings.is_empty() {
            args.push("--publish".to_string());
            args.push(port.clone());
            continue;
        }

        for binding in bindings {
            args.push("--publish".to_string());
            args.push(format_port_binding(port, binding));
        }
    }

    if host_config.auto_remove {
        args.push("--rm".to_string());
    }
}

fn append_healthcheck_runtime_args(args: &mut Vec<String>, healthcheck: &HealthConfig) {
    match healthcheck.test.as_slice() {
        [single] if single == "NONE" => args.push("--no-healthcheck".to_string()),
        [first, rest @ ..] if first == "CMD-SHELL" || first == "CMD" => {
            if !rest.is_empty() {
                args.push("--health-cmd".to_string());
                args.push(join_shell_words(rest));
            }
            if !healthcheck.interval.is_zero() {
                args.push("--health-interval".to_string());
                args.push(format_duration(healthcheck.interval));
            }
            if !healthcheck.timeout.is_zero() {
                args.push("--health-timeout".to_string());
                args.push(format_duration(healthcheck.timeout));
            }
            if !healthcheck.start_period.is_zero() {
                args.push("--health-start-period".to_string());
                args.push(format_duration(healthcheck.start_period));
            }
            if healthcheck.retries != 0 {
                args.push("--health-retries".to_string());
                args.push(healthcheck.retries.to_string());
            }
        }
        _ => {}
    }
}

fn append_entrypoint_args(args: &mut Vec<String>, config: &ContainerConfig) {
    if let Some(entrypoint) = config.entrypoint.first() {
        args.push("--entrypoint".to_string());
        args.push(entrypoint.clone());
    }
}

fn append_command_args(args: &mut Vec<String>, config: &ContainerConfig) {
    if let Some((_, entrypoint_rest)) = config.entrypoint.split_first() {
        args.extend(entrypoint_rest.iter().cloned());
    }
    args.extend(config.cmd.iter().cloned());
}

fn format_port_binding(port: &str, binding: &PortBinding) -> String {
    match (binding.host_ip.as_deref(), binding.host_port.as_deref()) {
        (Some(host_ip), Some(host_port)) if !host_ip.is_empty() && !host_port.is_empty() => {
            format!("{host_ip}:{host_port}:{port}")
        }
        (_, Some(host_port)) if !host_port.is_empty() => format!("{host_port}:{port}"),
        _ => port.to_string(),
    }
}

fn validate_supported_recreate(
    container_name: &str,
    host_config: &CliHostConfig,
) -> Result<(), DockerCliError> {
    if host_config.memory != 0 {
        return unsupported(container_name, "memory limits are not translated yet");
    }
    if host_config.nano_cpus != 0 {
        return unsupported(container_name, "CPU quota settings are not translated yet");
    }
    if host_config.cpu_shares != 0 {
        return unsupported(container_name, "CPU shares are not translated yet");
    }
    if !host_config.cpuset_cpus.is_empty() || !host_config.cpuset_mems.is_empty() {
        return unsupported(container_name, "CPU set constraints are not translated yet");
    }
    if host_config.devices.as_ref().is_some_and(|devices| !devices.is_empty()) {
        return unsupported(container_name, "device mappings are not translated yet");
    }
    if host_config
        .device_cgroup_rules
        .as_ref()
        .is_some_and(|rules| !rules.is_empty())
    {
        return unsupported(container_name, "device cgroup rules are not translated yet");
    }
    if host_config
        .device_requests
        .as_ref()
        .is_some_and(|requests| !requests.is_empty())
    {
        return unsupported(container_name, "device requests are not translated yet");
    }

    Ok(())
}

fn unsupported<T>(container_name: &str, detail: &str) -> Result<T, DockerCliError> {
    Err(DockerCliError::UnsupportedConfig {
        container: container_name.to_string(),
        detail: detail.to_string(),
    })
}

fn append_basic_create_args(args: &mut Vec<String>, config: &CliConfig, host_config: &CliHostConfig) {
    if !config.hostname.is_empty() {
        args.push("--hostname".to_string());
        args.push(config.hostname.clone());
    }
    if !config.domainname.is_empty() {
        args.push("--domainname".to_string());
        args.push(config.domainname.clone());
    }
    if !config.user.is_empty() {
        args.push("--user".to_string());
        args.push(config.user.clone());
    }
    if !config.working_dir.is_empty() {
        args.push("--workdir".to_string());
        args.push(config.working_dir.clone());
    }
    if config.tty {
        args.push("--tty".to_string());
    }
    if config.open_stdin {
        args.push("--interactive".to_string());
    }
    if let Some(stop_signal) = config.stop_signal.as_deref().filter(|value| !value.is_empty()) {
        args.push("--stop-signal".to_string());
        args.push(stop_signal.to_string());
    }

    for env in config.env.as_deref().unwrap_or(&[]) {
        args.push("--env".to_string());
        args.push(env.clone());
    }
    for (key, value) in config.labels.as_ref().into_iter().flatten() {
        args.push("--label".to_string());
        args.push(format!("{key}={value}"));
    }
    for port in config
        .exposed_ports
        .as_ref()
        .into_iter()
        .flat_map(|ports| ports.keys())
    {
        args.push("--expose".to_string());
        args.push(port.clone());
    }
    if let Some(healthcheck) = config.healthcheck.as_ref() {
        append_healthcheck_args(args, healthcheck);
    }

    if host_config.auto_remove {
        args.push("--rm".to_string());
    }
    if host_config.privileged {
        args.push("--privileged".to_string());
    }
    if host_config.readonly_rootfs {
        args.push("--read-only".to_string());
    }
    if host_config.publish_all_ports {
        args.push("--publish-all".to_string());
    }
    if let Some(policy) = host_config.restart_policy.as_ref() {
        append_restart_policy_args(args, policy);
    }
    if let Some(log_config) = host_config.log_config.as_ref() {
        append_log_config_args(args, log_config);
    }
    for link in host_config.links.as_deref().unwrap_or(&[]) {
        args.push("--link".to_string());
        args.push(link.clone());
    }
    for value in host_config.volumes_from.as_deref().unwrap_or(&[]) {
        args.push("--volumes-from".to_string());
        args.push(value.clone());
    }
    for value in host_config.cap_add.as_deref().unwrap_or(&[]) {
        args.push("--cap-add".to_string());
        args.push(value.clone());
    }
    for value in host_config.cap_drop.as_deref().unwrap_or(&[]) {
        args.push("--cap-drop".to_string());
        args.push(value.clone());
    }
    for value in host_config.dns.as_deref().unwrap_or(&[]) {
        args.push("--dns".to_string());
        args.push(value.clone());
    }
    for value in host_config.dns_search.as_deref().unwrap_or(&[]) {
        args.push("--dns-search".to_string());
        args.push(value.clone());
    }
    for value in host_config.dns_options.as_deref().unwrap_or(&[]) {
        args.push("--dns-option".to_string());
        args.push(value.clone());
    }
    for value in host_config.extra_hosts.as_deref().unwrap_or(&[]) {
        args.push("--add-host".to_string());
        args.push(value.clone());
    }
    for value in host_config.group_add.as_deref().unwrap_or(&[]) {
        args.push("--group-add".to_string());
        args.push(value.clone());
    }
    for value in host_config.security_opt.as_deref().unwrap_or(&[]) {
        args.push("--security-opt".to_string());
        args.push(value.clone());
    }
    for (target, options) in host_config.tmpfs.as_ref().into_iter().flatten() {
        args.push("--tmpfs".to_string());
        if options.is_empty() {
            args.push(target.clone());
        } else {
            args.push(format!("{target}:{options}"));
        }
    }
    for limit in host_config.ulimits.as_deref().unwrap_or(&[]) {
        args.push("--ulimit".to_string());
        args.push(format!("{}={}:{}", limit.name, limit.soft, limit.hard));
    }
    if !host_config.ipc_mode.is_empty() {
        args.push("--ipc".to_string());
        args.push(host_config.ipc_mode.clone());
    }
    if !host_config.pid_mode.is_empty() {
        args.push("--pid".to_string());
        args.push(host_config.pid_mode.clone());
    }
    if !host_config.userns_mode.is_empty() {
        args.push("--userns".to_string());
        args.push(host_config.userns_mode.clone());
    }
    if host_config.shm_size > 0 {
        args.push("--shm-size".to_string());
        args.push(host_config.shm_size.to_string());
    }

    for (port, bindings) in host_config.port_bindings.as_ref().into_iter().flatten() {
        if bindings.is_empty() {
            args.push("--publish".to_string());
            args.push(port.clone());
            continue;
        }

        for binding in bindings {
            args.push("--publish".to_string());
            args.push(format_port_publish(port, binding));
        }
    }
}

fn append_mount_args(
    args: &mut Vec<String>,
    container_name: &str,
    mounts: &[CliMount],
) -> Result<(), DockerCliError> {
    for mount in mounts {
        let spec = match mount.r#type.as_str() {
            "" => continue,
            "bind" => {
                if mount.source.is_empty() || mount.destination.is_empty() {
                    return unsupported(container_name, "bind mount source/destination missing");
                }
                let mut options = vec![
                    "type=bind".to_string(),
                    format!("src={}", mount.source),
                    format!("dst={}", mount.destination),
                ];
                if !mount.rw {
                    options.push("readonly".to_string());
                }
                if !mount.propagation.is_empty() {
                    options.push(format!("bind-propagation={}", mount.propagation));
                }
                options.join(",")
            }
            "volume" => {
                let Some(name) = mount.name.as_deref().filter(|value| !value.is_empty()) else {
                    return unsupported(container_name, "named volume mount missing `Name`");
                };
                if mount.destination.is_empty() {
                    return unsupported(container_name, "volume destination missing");
                }
                let mut options = vec![
                    "type=volume".to_string(),
                    format!("src={name}"),
                    format!("dst={}", mount.destination),
                ];
                if !mount.rw {
                    options.push("readonly".to_string());
                }
                options.join(",")
            }
            "tmpfs" => {
                if mount.destination.is_empty() {
                    return unsupported(container_name, "tmpfs destination missing");
                }
                format!("type=tmpfs,dst={}", mount.destination)
            }
            other => {
                return unsupported(
                    container_name,
                    &format!("mount type `{other}` is not translated yet"),
                )
            }
        };

        args.push("--mount".to_string());
        args.push(spec);
    }

    Ok(())
}

fn append_healthcheck_args(args: &mut Vec<String>, healthcheck: &CliHealthcheck) {
    match healthcheck.test.as_slice() {
        [single] if single == "NONE" => args.push("--no-healthcheck".to_string()),
        [first, rest @ ..] if first == "CMD-SHELL" || first == "CMD" => {
            if !rest.is_empty() {
                args.push("--health-cmd".to_string());
                args.push(join_shell_words(rest));
            }
            if healthcheck.interval != 0 {
                args.push("--health-interval".to_string());
                args.push(format_duration(duration_from_nanos(healthcheck.interval)));
            }
            if healthcheck.timeout != 0 {
                args.push("--health-timeout".to_string());
                args.push(format_duration(duration_from_nanos(healthcheck.timeout)));
            }
            if healthcheck.start_period != 0 {
                args.push("--health-start-period".to_string());
                args.push(format_duration(duration_from_nanos(healthcheck.start_period)));
            }
            if healthcheck.retries != 0 {
                args.push("--health-retries".to_string());
                args.push(healthcheck.retries.to_string());
            }
        }
        _ => {}
    }
}

fn append_restart_policy_args(args: &mut Vec<String>, policy: &CliRestartPolicy) {
    if policy.name.is_empty() || policy.name == "no" {
        return;
    }

    args.push("--restart".to_string());
    if policy.name == "on-failure" && policy.maximum_retry_count > 0 {
        args.push(format!("{}:{}", policy.name, policy.maximum_retry_count));
    } else {
        args.push(policy.name.clone());
    }
}

fn append_log_config_args(args: &mut Vec<String>, log_config: &CliLogConfig) {
    if !log_config.r#type.is_empty() {
        args.push("--log-driver".to_string());
        args.push(log_config.r#type.clone());
    }

    for (key, value) in log_config.config.as_ref().into_iter().flatten() {
        args.push("--log-opt".to_string());
        args.push(format!("{key}={value}"));
    }
}

fn append_entrypoint_option(args: &mut Vec<String>, config: &CliConfig) {
    let entrypoint = config.entrypoint.as_deref().unwrap_or(&[]);
    if let Some((entrypoint_bin, _)) = entrypoint.split_first() {
        args.push("--entrypoint".to_string());
        args.push(entrypoint_bin.clone());
    }
}

fn append_command_tail(args: &mut Vec<String>, config: &CliConfig) {
    let entrypoint = config.entrypoint.as_deref().unwrap_or(&[]);
    if let Some((_, entrypoint_rest)) = entrypoint.split_first() {
        args.extend(entrypoint_rest.iter().cloned());
    }
    args.extend(config.cmd.as_deref().unwrap_or(&[]).iter().cloned());
}

fn determine_primary_network(
    host_config: &CliHostConfig,
    inspect: &CliContainerInspect,
) -> Option<NetworkConnectPlan> {
    let mut endpoints = inspect
        .network_settings
        .as_ref()
        .map(|settings| normalized_network_endpoints(settings, inspect.id.as_str()))
        .unwrap_or_default();

    if endpoints.is_empty() {
        return None;
    }

    match host_config.network_mode.as_str() {
        "" | "default" => Some(endpoints.remove(0)),
        "host" => None,
        mode if mode.starts_with("container:") => None,
        mode => endpoints
            .iter()
            .position(|endpoint| endpoint.name == mode)
            .map(|index| endpoints.remove(index))
            .or_else(|| Some(endpoints.remove(0))),
    }
}

fn normalized_network_endpoints(
    settings: &CliNetworkSettings,
    container_id: &str,
) -> Vec<NetworkConnectPlan> {
    let Some(networks) = settings.networks.as_ref() else {
        return Vec::new();
    };

    let config = NetworkingConfig {
        endpoints: networks
            .iter()
            .map(|(name, endpoint)| {
                (
                    name.clone(),
                    NetworkEndpoint {
                        aliases: endpoint.aliases.clone().unwrap_or_default(),
                    },
                )
            })
            .collect(),
    };
    let config = normalize_network_config(config, &ContainerID::new(container_id).short_id());

    let mut names = config.endpoints.keys().cloned().collect::<Vec<_>>();
    names.sort();
    names
        .into_iter()
        .map(|name| NetworkConnectPlan {
            aliases: config
                .endpoints
                .get(&name)
                .map(|endpoint| endpoint.aliases.clone())
                .unwrap_or_default(),
            name,
        })
        .collect()
}

fn parse_network_mode(mode: &str) -> NetworkMode {
    match mode {
        "" | "default" => NetworkMode::Default,
        "host" => NetworkMode::Host,
        other if other.starts_with("container:") => {
            NetworkMode::Container(other.trim_start_matches("container:").to_string())
        }
        other => NetworkMode::Other(other.to_string()),
    }
}

fn empty_to_none(value: Option<&str>) -> Option<String> {
    value.filter(|value| !value.is_empty()).map(ToOwned::to_owned)
}

fn duration_from_nanos(value: i64) -> Duration {
    Duration::from_nanos(value.max(0) as u64)
}

fn format_duration(duration: Duration) -> String {
    format!("{}ns", duration.as_nanos())
}

fn format_port_publish(port: &str, binding: &CliPortBinding) -> String {
    match (binding.host_ip.as_deref(), binding.host_port.as_deref()) {
        (Some(host_ip), Some(host_port)) if !host_ip.is_empty() && !host_port.is_empty() => {
            format!("{host_ip}:{host_port}:{port}")
        }
        (_, Some(host_port)) if !host_port.is_empty() => format!("{host_port}:{port}"),
        _ => port.to_string(),
    }
}

fn join_shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| shell_escape(word))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"-_./:=,@".contains(&byte))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn trim_container_name(name: &str) -> &str {
    name.strip_prefix('/').unwrap_or(name)
}

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

/// Build the legacy Docker container status set used when listing containers.
///
/// The Go client always included `running`, and conditionally added
/// `created`/`exited` and `restarting` based on the corresponding flags.
#[must_use]
pub fn container_list_statuses(
    include_stopped: bool,
    include_restarting: bool,
) -> Vec<&'static str> {
    let mut statuses = vec!["running"];

    if include_stopped {
        statuses.push("created");
        statuses.push("exited");
    }

    if include_restarting {
        statuses.push("restarting");
    }

    statuses
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    const SAMPLE_INSPECT: &str = r#"
    {
      "Id": "1234567890ab1234567890ab1234567890ab1234567890ab1234567890abcd",
      "Name": "/demo",
      "Created": "2026-06-20T11:00:00Z",
      "Image": "sha256:current",
      "State": {
        "Running": true,
        "Restarting": false
      },
      "Config": {
        "Image": "registry.example.com/team/app:latest",
        "Labels": {
          "com.example.role": "api"
        },
        "WorkingDir": "/srv/app",
        "User": "app",
        "Entrypoint": ["/bin/sh", "-c"],
        "Cmd": ["echo hi"],
        "Env": ["A=1"],
        "ExposedPorts": {
          "80/tcp": {}
        },
        "Healthcheck": {
          "Test": ["CMD-SHELL", "curl -f http://127.0.0.1/health || exit 1"],
          "Interval": 30000000000,
          "Timeout": 5000000000,
          "StartPeriod": 10000000000,
          "Retries": 3
        },
        "Hostname": "demo-host",
        "StopSignal": "SIGTERM"
      },
      "HostConfig": {
        "NetworkMode": "customnet",
        "PortBindings": {
          "80/tcp": [
            {
              "HostIp": "127.0.0.1",
              "HostPort": "8080"
            }
          ]
        },
        "RestartPolicy": {
          "Name": "unless-stopped",
          "MaximumRetryCount": 0
        },
        "AutoRemove": false
      },
      "NetworkSettings": {
        "Networks": {
          "customnet": {
            "Aliases": ["1234567890ab", "demo", "demo-api"]
          },
          "metrics": {
            "Aliases": ["1234567890ab", "metrics-sidecar"]
          }
        }
      },
      "Mounts": [
        {
          "Type": "bind",
          "Source": "/host/config",
          "Destination": "/etc/demo/config",
          "Mode": "ro",
          "RW": false,
          "Propagation": "rprivate"
        },
        {
          "Type": "volume",
          "Name": "demo-data",
          "Source": "/var/lib/docker/volumes/demo-data/_data",
          "Destination": "/var/lib/demo",
          "Mode": "",
          "RW": true,
          "Propagation": ""
        }
      ]
    }
    "#;

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

    fn runtime_container() -> Container {
        Container::new(
            ContainerInspect {
                id: ContainerID::new("old-container-id"),
                name: "/demo".to_string(),
                image: ImageID::new("sha256:old-image"),
                created: "2026-06-20T11:00:00Z".to_string(),
                state: ContainerState {
                    running: true,
                    restarting: false,
                },
                config: Some(ContainerConfig {
                    image: "registry.example.com/team/app:latest".to_string(),
                    labels: BTreeMap::from([(
                        "com.centurylinklabs.watchtower.stop-signal".to_string(),
                        "SIGUSR1".to_string(),
                    )]),
                    working_dir: "/srv/app".to_string(),
                    user: "app".to_string(),
                    entrypoint: vec!["/bin/sh".to_string(), "-c".to_string()],
                    cmd: vec!["echo hi".to_string()],
                    env: vec!["A=1".to_string()],
                    volumes: BTreeSet::from(["/data".to_string()]),
                    exposed_ports: Some(BTreeSet::from(["80/tcp".to_string()])),
                    healthcheck: Some(HealthConfig {
                        test: vec!["CMD-SHELL".to_string(), "echo ok".to_string()],
                        interval: Duration::from_secs(30),
                        timeout: Duration::from_secs(5),
                        start_period: Duration::from_secs(10),
                        retries: 3,
                    }),
                    hostname: "demo-host".to_string(),
                }),
                host_config: Some(HostConfig {
                    links: vec!["/redis:/demo".to_string()],
                    network_mode: NetworkMode::Default,
                    port_bindings: BTreeMap::from([(
                        "80/tcp".to_string(),
                        vec![PortBinding {
                            host_ip: Some("127.0.0.1".to_string()),
                            host_port: Some("8080".to_string()),
                        }],
                    )]),
                    auto_remove: false,
                }),
                network_settings: Some(HashMap::from([(
                    "bridge".to_string(),
                    NetworkEndpoint {
                        aliases: vec![
                            "old-containe".to_string(),
                            "demo".to_string(),
                            "demo-api".to_string(),
                        ],
                    },
                )])),
            },
            Some(ImageInspect {
                id: ImageID::new("sha256:old-image"),
                config: ContainerConfig {
                    image: "registry.example.com/team/app:latest".to_string(),
                    labels: BTreeMap::new(),
                    working_dir: String::new(),
                    user: String::new(),
                    entrypoint: Vec::new(),
                    cmd: Vec::new(),
                    env: Vec::new(),
                    volumes: BTreeSet::new(),
                    exposed_ports: Some(BTreeSet::new()),
                    healthcheck: None,
                    hostname: String::new(),
                },
            }),
        )
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("watchtower-ng-{label}-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_fake_docker(dir: &Path, body: &str) -> PathBuf {
        let script_path = dir.join("fake-docker.sh");
        let script = format!("#!/bin/sh\nset -eu\n{body}\n");
        fs::write(&script_path, script).expect("script");

        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).expect("chmod");
        }

        script_path
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

    #[test]
    fn container_list_statuses_matches_legacy_flags() {
        assert_eq!(container_list_statuses(false, false), vec!["running"]);
        assert_eq!(
            container_list_statuses(true, false),
            vec!["running", "created", "exited"]
        );
        assert_eq!(
            container_list_statuses(false, true),
            vec!["running", "restarting"]
        );
        assert_eq!(
            container_list_statuses(true, true),
            vec!["running", "created", "exited", "restarting"]
        );
    }

    #[test]
    fn create_plan_translates_common_container_inspect_fields() {
        let inspect: CliContainerInspect =
            serde_json::from_str(SAMPLE_INSPECT).expect("inspect json should parse");

        let plan = CreatePlan::from_inspect("/demo", &inspect).expect("create plan");

        assert_eq!(plan.args[0..4], ["container", "create", "--name", "demo"]);
        assert!(plan.args.contains(&"--hostname".to_string()));
        assert!(plan.args.contains(&"demo-host".to_string()));
        assert!(plan.args.contains(&"--user".to_string()));
        assert!(plan.args.contains(&"app".to_string()));
        assert!(plan.args.contains(&"--workdir".to_string()));
        assert!(plan.args.contains(&"/srv/app".to_string()));
        assert!(plan.args.contains(&"--env".to_string()));
        assert!(plan.args.contains(&"A=1".to_string()));
        assert!(plan.args.contains(&"--label".to_string()));
        assert!(plan.args.contains(&"com.example.role=api".to_string()));
        assert!(plan.args.contains(&"--publish".to_string()));
        assert!(
            plan.args
                .contains(&"127.0.0.1:8080:80/tcp".to_string())
        );
        assert!(plan.args.contains(&"--mount".to_string()));
        assert!(
            plan.args.contains(
                &"type=bind,src=/host/config,dst=/etc/demo/config,readonly,bind-propagation=rprivate"
                    .to_string()
            )
        );
        assert!(
            plan.args
                .contains(&"type=volume,src=demo-data,dst=/var/lib/demo".to_string())
        );
        assert!(plan.args.contains(&"--restart".to_string()));
        assert!(plan.args.contains(&"unless-stopped".to_string()));
        assert!(plan.args.contains(&"--network".to_string()));
        assert!(plan.args.contains(&"customnet".to_string()));
        assert!(plan.args.contains(&"--network-alias".to_string()));
        assert!(plan.args.contains(&"demo".to_string()));
        assert!(plan.args.contains(&"demo-api".to_string()));
        assert!(!plan.args.contains(&"1234567890ab".to_string()));
        assert_eq!(
            plan.extra_networks,
            vec![NetworkConnectPlan {
                name: "metrics".to_string(),
                aliases: vec!["metrics-sidecar".to_string()],
            }]
        );

        let image_index = plan
            .args
            .iter()
            .position(|arg| arg == "registry.example.com/team/app:latest")
            .expect("image arg");
        let entrypoint_index = plan
            .args
            .iter()
            .position(|arg| arg == "--entrypoint")
            .expect("entrypoint flag");
        assert!(entrypoint_index < image_index);
        assert_eq!(plan.args[image_index + 1], "-c");
        assert_eq!(plan.args[image_index + 2], "echo hi");
    }

    #[test]
    fn create_plan_rejects_untranslated_resource_limits() {
        let inspect: CliContainerInspect = serde_json::from_str(
            &SAMPLE_INSPECT.replace("\"AutoRemove\": false", "\"AutoRemove\": false, \"Memory\": 1024"),
        )
        .expect("inspect json should parse");

        let err = CreatePlan::from_inspect("/demo", &inspect).expect_err("memory limit must fail closed");

        assert!(matches!(
            err,
            DockerCliError::UnsupportedConfig { ref detail, .. } if detail.contains("memory limits")
        ));
    }

    #[test]
    fn inspect_conversion_preserves_runtime_state_and_network_aliases() {
        let inspect: CliContainerInspect =
            serde_json::from_str(SAMPLE_INSPECT).expect("inspect json should parse");

        let converted = inspect.into_container_inspect();

        assert_eq!(converted.id, ContainerID::new("1234567890ab1234567890ab1234567890ab1234567890ab1234567890abcd"));
        assert_eq!(converted.name, "/demo");
        assert!(converted.state.running);
        assert!(!converted.state.restarting);
        assert_eq!(
            converted
                .network_settings
                .as_ref()
                .and_then(|networks| networks.get("customnet"))
                .map(|endpoint| endpoint.aliases.clone()),
            Some(vec![
                "1234567890ab".to_string(),
                "demo".to_string(),
                "demo-api".to_string()
            ])
        );
    }

    #[test]
    fn stale_check_keeps_has_new_image_path_when_no_pull_is_enabled() {
        let dir = unique_temp_dir("no-pull");
        let log_path = dir.join("docker.log");
        let script_path = write_fake_docker(
            &dir,
            &format!(
                "printf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '[{{\"Id\":\"sha256:new-image\",\"Config\":{{\"Image\":\"registry.example.com/team/app:latest\"}},\"RepoDigests\":[\"registry.example.com/team/app@sha256:new-image\"]}}]'\n  exit 0\nfi\nexit 1",
                log_path.display()
            ),
        );
        let adapter = DockerCliAdapter::with_binary(script_path.display().to_string());
        let container = runtime_container();
        let params = UpdateParams {
            no_pull: true,
            ..UpdateParams::default()
        };

        let (stale, latest_image) = adapter
            .is_container_stale(&container, &params)
            .expect("stale check");

        assert!(stale);
        assert_eq!(latest_image, ImageID::new("sha256:new-image"));

        let log = fs::read_to_string(&log_path).expect("docker log");
        assert!(log.contains("image inspect registry.example.com/team/app:latest"));
        assert!(!log.contains("pull registry.example.com/team/app:latest"));
    }

    #[test]
    fn start_container_recreates_from_snapshot_without_old_container_inspect_or_remove() {
        let dir = unique_temp_dir("start");
        let log_path = dir.join("docker.log");
        let script_path = write_fake_docker(
            &dir,
            &format!(
                "printf '%s\\n' \"$*\" >> '{}'\ncase \"$1 $2\" in\n  'container create') printf '%s\\n' 'new-container-id' ;;\n  'network disconnect') exit 0 ;;\n  'network connect') exit 0 ;;\n  'start new-container-id') exit 0 ;;\n  'inspect '*|'rm --force') echo 'unexpected legacy mismatch' >&2; exit 91 ;;\n  *) exit 0 ;;\nesac",
                log_path.display()
            ),
        );
        let adapter = DockerCliAdapter::with_binary(script_path.display().to_string());
        let container = runtime_container();

        let created_id = adapter.start_container(&container).expect("start container");

        assert_eq!(created_id, ContainerID::new("new-container-id"));

        let log = fs::read_to_string(&log_path).expect("docker log");
        assert!(log.contains("container create"));
        assert!(log.contains("network disconnect bridge new-container-id --force"));
        assert!(log.contains("network connect --alias demo --alias demo-api bridge new-container-id"));
        assert!(log.contains("start new-container-id"));
        assert!(!log.contains("inspect old-container-id"));
        assert!(!log.contains("rm --force old-container-id"));
    }

    #[test]
    fn get_container_rewrites_container_network_parent_id_to_parent_name() {
        let dir = unique_temp_dir("network-parent");
        let log_path = dir.join("docker.log");
        let script_path = write_fake_docker(
            &dir,
            &format!(
                "printf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"inspect\" ] && [ \"$2\" = \"child-id\" ]; then\n  printf '%s\\n' '[{{\"Id\":\"child-id\",\"Name\":\"/child\",\"Created\":\"2026-06-20T11:00:00Z\",\"Image\":\"sha256:current\",\"State\":{{\"Running\":true,\"Restarting\":false}},\"Config\":{{\"Image\":\"registry.example.com/team/app:latest\",\"Labels\":{{}}}},\"HostConfig\":{{\"NetworkMode\":\"container:parent-id\",\"AutoRemove\":false}},\"NetworkSettings\":{{\"Networks\":{{}}}},\"Mounts\":[]}}]'\n  exit 0\nfi\nif [ \"$1\" = \"inspect\" ] && [ \"$2\" = \"parent-id\" ]; then\n  printf '%s\\n' '[{{\"Id\":\"parent-id\",\"Name\":\"/parent\",\"Created\":\"2026-06-20T11:00:00Z\",\"Image\":\"sha256:parent\",\"State\":{{\"Running\":true,\"Restarting\":false}},\"Config\":{{\"Image\":\"registry.example.com/team/parent:latest\",\"Labels\":{{}}}},\"HostConfig\":{{\"NetworkMode\":\"default\",\"AutoRemove\":false}},\"NetworkSettings\":{{\"Networks\":{{}}}},\"Mounts\":[]}}]'\n  exit 0\nfi\nif [ \"$1\" = \"image\" ] && [ \"$2\" = \"inspect\" ]; then\n  printf '%s\\n' '[{{\"Id\":\"sha256:current\",\"Config\":{{\"Image\":\"registry.example.com/team/app:latest\"}},\"RepoDigests\":[]}}]'\n  exit 0\nfi\nexit 1",
                log_path.display()
            ),
        );
        let adapter = DockerCliAdapter::with_binary(script_path.display().to_string());

        let container = adapter
            .get_container(&ContainerID::new("child-id"))
            .expect("container");

        assert_eq!(
            container
                .container_info()
                .and_then(|info| info.host_config.as_ref())
                .and_then(|host_config| host_config.network_mode.connected_container()),
            Some("/parent")
        );

        let log = fs::read_to_string(&log_path).expect("docker log");
        assert!(log.contains("inspect child-id"));
        assert!(log.contains("inspect parent-id"));
    }
}
