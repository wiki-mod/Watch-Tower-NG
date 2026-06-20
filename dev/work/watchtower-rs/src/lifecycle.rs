#![forbid(unsafe_code)]

//! Lifecycle hook helpers translated from the legacy Go implementation.
//!
//! The functions keep Docker interaction behind a small client trait so the
//! hook behavior can be tested without wiring in HTTP/runtime code.

use crate::container::Container;
use crate::types::{ContainerID, UpdateParams};

const CHECK_TIMEOUT_MINUTES: i64 = 1;

/// Minimal client surface required by the lifecycle hook helpers.
pub trait LifecycleClient {
    type Error;

    fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error>;
    fn get_container(&self, container_id: &ContainerID)
        -> std::result::Result<Container, Self::Error>;
    fn execute_command(
        &self,
        container_id: &ContainerID,
        command: &str,
        timeout_minutes: i64,
    ) -> std::result::Result<bool, Self::Error>;
}

/// Per-container hook execution record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookExecution {
    pub container_id: ContainerID,
    pub container_name: String,
    pub outcome: HookOutcome,
}

/// Explicit lifecycle hook outcome, including documented no-op paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOutcome {
    Executed { skip_update: bool },
    Skipped(HookSkipReason),
}

/// Reason why a lifecycle hook did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookSkipReason {
    MissingCommand,
    ContainerNotRunning,
    ContainerRestarting,
}

pub fn execute_pre_checks<C: LifecycleClient>(
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<Vec<HookExecution>, C::Error> {
    execute_checks(client, params, Container::get_lifecycle_pre_check_command)
}

pub fn execute_post_checks<C: LifecycleClient>(
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<Vec<HookExecution>, C::Error> {
    execute_checks(client, params, Container::get_lifecycle_post_check_command)
}

pub fn execute_pre_check_command<C: LifecycleClient>(
    client: &C,
    container: &Container,
) -> std::result::Result<HookOutcome, C::Error> {
    execute_check_command(
        client,
        container,
        container.get_lifecycle_pre_check_command(),
    )
}

pub fn execute_post_check_command<C: LifecycleClient>(
    client: &C,
    container: &Container,
) -> std::result::Result<HookOutcome, C::Error> {
    execute_check_command(
        client,
        container,
        container.get_lifecycle_post_check_command(),
    )
}

pub fn execute_pre_update_command<C: LifecycleClient>(
    client: &C,
    container: &Container,
) -> std::result::Result<HookOutcome, C::Error> {
    let command = container.get_lifecycle_pre_update_command();
    if command.is_empty() {
        return Ok(HookOutcome::Skipped(HookSkipReason::MissingCommand));
    }

    if !container.is_running() {
        return Ok(HookOutcome::Skipped(HookSkipReason::ContainerNotRunning));
    }

    if container.is_restarting() {
        return Ok(HookOutcome::Skipped(HookSkipReason::ContainerRestarting));
    }

    execute_command(
        client,
        container.id(),
        &command,
        container.pre_update_timeout(),
    )
}

pub fn execute_post_update_command<C: LifecycleClient>(
    client: &C,
    new_container_id: &ContainerID,
) -> std::result::Result<HookOutcome, C::Error> {
    let container = client.get_container(new_container_id)?;
    let command = container.get_lifecycle_post_update_command();
    if command.is_empty() {
        return Ok(HookOutcome::Skipped(HookSkipReason::MissingCommand));
    }

    execute_command(
        client,
        container.id(),
        &command,
        container.post_update_timeout(),
    )
}

fn execute_checks<C: LifecycleClient>(
    client: &C,
    params: &UpdateParams,
    command_selector: fn(&Container) -> String,
) -> std::result::Result<Vec<HookExecution>, C::Error> {
    filtered_containers(client, params)?
        .into_iter()
        .map(|container| {
            let outcome = execute_check_command(client, &container, command_selector(&container))?;
            Ok(HookExecution {
                container_id: container.id().clone(),
                container_name: container.name().to_string(),
                outcome,
            })
        })
        .collect()
}

fn filtered_containers<C: LifecycleClient>(
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<Vec<Container>, C::Error> {
    let mut containers = client.list_containers()?;
    containers.retain(|container| params.matches(container));
    Ok(containers)
}

fn execute_check_command<C: LifecycleClient>(
    client: &C,
    container: &Container,
    command: String,
) -> std::result::Result<HookOutcome, C::Error> {
    if command.is_empty() {
        return Ok(HookOutcome::Skipped(HookSkipReason::MissingCommand));
    }

    execute_command(client, container.id(), &command, CHECK_TIMEOUT_MINUTES)
}

fn execute_command<C: LifecycleClient>(
    client: &C,
    container_id: &ContainerID,
    command: &str,
    timeout_minutes: i64,
) -> std::result::Result<HookOutcome, C::Error> {
    let skip_update = client.execute_command(container_id, command, timeout_minutes)?;
    Ok(HookOutcome::Executed { skip_update })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, VecDeque};

    use crate::container::{ContainerConfig, ContainerInspect, ContainerState};
    use crate::types::ImageID;

    const PRE_CHECK_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-check";
    const POST_CHECK_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.post-check";
    const PRE_UPDATE_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-update";
    const POST_UPDATE_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.post-update";
    const PRE_UPDATE_TIMEOUT_LABEL: &str =
        "com.centurylinklabs.watchtower.lifecycle.pre-update-timeout";
    const POST_UPDATE_TIMEOUT_LABEL: &str =
        "com.centurylinklabs.watchtower.lifecycle.post-update-timeout";

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum MockError {
        ListFailed,
        ContainerLookupFailed,
        CommandFailed,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ExecCall {
        container_id: ContainerID,
        command: String,
        timeout_minutes: i64,
    }

    struct MockClient {
        list_result: std::result::Result<Vec<Container>, MockError>,
        get_results: BTreeMap<String, std::result::Result<Container, MockError>>,
        exec_results: RefCell<VecDeque<std::result::Result<bool, MockError>>>,
        exec_calls: RefCell<Vec<ExecCall>>,
    }

    impl MockClient {
        fn new(list_result: std::result::Result<Vec<Container>, MockError>) -> Self {
            Self {
                list_result,
                get_results: BTreeMap::new(),
                exec_results: RefCell::new(VecDeque::new()),
                exec_calls: RefCell::new(Vec::new()),
            }
        }

        fn with_get_result(
            mut self,
            container_id: &ContainerID,
            result: std::result::Result<Container, MockError>,
        ) -> Self {
            self.get_results
                .insert(container_id.as_str().to_string(), result);
            self
        }

        fn with_exec_results(
            self,
            exec_results: impl IntoIterator<Item = std::result::Result<bool, MockError>>,
        ) -> Self {
            self.exec_results.borrow_mut().extend(exec_results);
            self
        }

        fn exec_calls(&self) -> Vec<ExecCall> {
            self.exec_calls.borrow().clone()
        }
    }

    impl LifecycleClient for MockClient {
        type Error = MockError;

        fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error> {
            self.list_result.clone()
        }

        fn get_container(
            &self,
            container_id: &ContainerID,
        ) -> std::result::Result<Container, Self::Error> {
            self.get_results
                .get(container_id.as_str())
                .cloned()
                .unwrap_or(Err(MockError::ContainerLookupFailed))
        }

        fn execute_command(
            &self,
            container_id: &ContainerID,
            command: &str,
            timeout_minutes: i64,
        ) -> std::result::Result<bool, Self::Error> {
            self.exec_calls.borrow_mut().push(ExecCall {
                container_id: container_id.clone(),
                command: command.to_string(),
                timeout_minutes,
            });
            self.exec_results
                .borrow_mut()
                .pop_front()
                .unwrap_or(Ok(false))
        }
    }

    fn lifecycle_container(
        id: &str,
        name: &str,
        labels: &[(&str, &str)],
        running: bool,
        restarting: bool,
    ) -> Container {
        Container::new(
            ContainerInspect {
                id: ContainerID::new(id),
                name: name.to_string(),
                image: ImageID::new("sha256:image"),
                created: "2024-06-18T12:00:00Z".to_string(),
                state: ContainerState {
                    running,
                    restarting,
                },
                config: Some(ContainerConfig {
                    image: "repo/image:latest".to_string(),
                    labels: labels
                        .iter()
                        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                        .collect(),
                    ..ContainerConfig::default()
                }),
                host_config: None,
                network_settings: None,
            },
            None,
        )
    }

    #[test]
    fn execute_pre_checks_filters_containers_and_uses_fixed_timeout() {
        let matched = lifecycle_container(
            "container-alpha",
            "alpha",
            &[(PRE_CHECK_LABEL, "echo pre-check")],
            true,
            false,
        );
        let skipped = lifecycle_container("container-beta", "beta", &[], true, false);
        let client = MockClient::new(Ok(vec![matched.clone(), skipped]))
            .with_exec_results([Ok(false)]);
        let params = UpdateParams::new().with_filter(|container| container.name() == "alpha");

        let executions = execute_pre_checks(&client, &params).expect("pre-checks should succeed");

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].container_id, matched.id().clone());
        assert_eq!(
            executions[0].outcome,
            HookOutcome::Executed { skip_update: false }
        );
        assert_eq!(
            client.exec_calls(),
            vec![ExecCall {
                container_id: matched.id().clone(),
                command: "echo pre-check".to_string(),
                timeout_minutes: CHECK_TIMEOUT_MINUTES,
            }]
        );
    }

    #[test]
    fn execute_post_checks_returns_list_error_instead_of_swallowing_it() {
        let client = MockClient::new(Err(MockError::ListFailed));

        let error = execute_post_checks(&client, &UpdateParams::new())
            .expect_err("post-checks should return the list error");

        assert_eq!(error, MockError::ListFailed);
    }

    #[test]
    fn execute_pre_update_command_returns_documented_no_op_paths() {
        let missing = lifecycle_container("container-missing", "missing", &[], true, false);
        let stopped = lifecycle_container(
            "container-stopped",
            "stopped",
            &[(PRE_UPDATE_LABEL, "echo pre-update")],
            false,
            false,
        );
        let restarting = lifecycle_container(
            "container-restarting",
            "restarting",
            &[(PRE_UPDATE_LABEL, "echo pre-update")],
            true,
            true,
        );
        let client = MockClient::new(Ok(Vec::new()));

        assert_eq!(
            execute_pre_update_command(&client, &missing).expect("missing command is a no-op"),
            HookOutcome::Skipped(HookSkipReason::MissingCommand)
        );
        assert_eq!(
            execute_pre_update_command(&client, &stopped).expect("stopped container is a no-op"),
            HookOutcome::Skipped(HookSkipReason::ContainerNotRunning)
        );
        assert_eq!(
            execute_pre_update_command(&client, &restarting)
                .expect("restarting container is a no-op"),
            HookOutcome::Skipped(HookSkipReason::ContainerRestarting)
        );
    }

    #[test]
    fn execute_pre_update_command_uses_container_timeout_and_skip_update_flag() {
        let container = lifecycle_container(
            "container-pre-update",
            "pre-update",
            &[
                (PRE_UPDATE_LABEL, "echo pre-update"),
                (PRE_UPDATE_TIMEOUT_LABEL, "7"),
            ],
            true,
            false,
        );
        let client = MockClient::new(Ok(Vec::new())).with_exec_results([Ok(true)]);

        let outcome =
            execute_pre_update_command(&client, &container).expect("pre-update should execute");

        assert_eq!(outcome, HookOutcome::Executed { skip_update: true });
        assert_eq!(
            client.exec_calls(),
            vec![ExecCall {
                container_id: container.id().clone(),
                command: "echo pre-update".to_string(),
                timeout_minutes: 7,
            }]
        );
    }

    #[test]
    fn execute_post_update_command_fetches_container_and_uses_post_timeout() {
        let container_id = ContainerID::new("container-post-update");
        let container = lifecycle_container(
            container_id.as_str(),
            "post-update",
            &[
                (POST_UPDATE_LABEL, "echo post-update"),
                (POST_UPDATE_TIMEOUT_LABEL, "11"),
            ],
            true,
            false,
        );
        let client = MockClient::new(Ok(Vec::new()))
            .with_get_result(&container_id, Ok(container.clone()))
            .with_exec_results([Ok(false)]);

        let outcome =
            execute_post_update_command(&client, &container_id).expect("post-update should run");

        assert_eq!(outcome, HookOutcome::Executed { skip_update: false });
        assert_eq!(
            client.exec_calls(),
            vec![ExecCall {
                container_id,
                command: "echo post-update".to_string(),
                timeout_minutes: 11,
            }]
        );
    }

    #[test]
    fn execute_post_update_command_propagates_command_errors() {
        let container_id = ContainerID::new("container-post-update-error");
        let container = lifecycle_container(
            container_id.as_str(),
            "post-update-error",
            &[(POST_CHECK_LABEL, "unused"), (POST_UPDATE_LABEL, "echo post-update")],
            true,
            false,
        );
        let client = MockClient::new(Ok(Vec::new()))
            .with_get_result(&container_id, Ok(container))
            .with_exec_results([Err(MockError::CommandFailed)]);

        let error = execute_post_update_command(&client, &container_id)
            .expect_err("command errors must propagate");

        assert_eq!(error, MockError::CommandFailed);
    }
}
