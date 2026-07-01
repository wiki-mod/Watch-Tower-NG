#![forbid(unsafe_code)]

use crate::container::Container;
use crate::types::{ContainerID, UpdateParams};
use tracing::{debug, error};

const CHECK_TIMEOUT_MINUTES: i64 = 1;

pub trait LifecycleClient {
    type Error;

    fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error>;
    fn get_container(
        &self,
        container_id: &ContainerID,
    ) -> std::result::Result<Container, Self::Error>;
    fn execute_command(
        &self,
        container_id: &ContainerID,
        command: &str,
        timeout_minutes: i64,
    ) -> std::result::Result<bool, Self::Error>;
}


/// ExecutePreChecks tries to run the pre-check lifecycle hook for all
/// containers included by the current filter.
pub fn execute_pre_checks<C: LifecycleClient>(
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<(), C::Error> {
    let mut containers = match client.list_containers() {
        Ok(containers) => containers,
        Err(_) => return Ok(()),
    };

    containers.retain(|container| params.matches(container));
    for current_container in containers {
        execute_pre_check_command(client, &current_container);
    }

    Ok(())
}

/// ExecutePostChecks tries to run the post-check lifecycle hook for all
/// containers included by the current filter.
pub fn execute_post_checks<C: LifecycleClient>(
    client: &C,
    params: &UpdateParams,
) -> std::result::Result<(), C::Error> {
    let mut containers = match client.list_containers() {
        Ok(containers) => containers,
        Err(_) => return Ok(()),
    };

    containers.retain(|container| params.matches(container));
    for current_container in containers {
        execute_post_check_command(client, &current_container);
    }

    Ok(())
}

/// ExecutePreCheckCommand tries to run the pre-check lifecycle hook for a
/// single container.
pub fn execute_pre_check_command<C: LifecycleClient>(client: &C, container: &Container) {
    let command = container.get_lifecycle_pre_check_command();
    if command.is_empty() {
        debug!(
            container = container.name(),
            "No pre-check command supplied. Skipping"
        );
        return;
    }

    debug!(container = container.name(), "Executing pre-check command.");
    if client
        .execute_command(container.id(), &command, CHECK_TIMEOUT_MINUTES)
        .is_err()
    {
        error!(container = container.name(), "pre-check command failed");
    }
}

/// ExecutePostCheckCommand tries to run the post-check lifecycle hook for a
/// single container.
pub fn execute_post_check_command<C: LifecycleClient>(client: &C, container: &Container) {
    let command = container.get_lifecycle_post_check_command();
    if command.is_empty() {
        debug!(
            container = container.name(),
            "No post-check command supplied. Skipping"
        );
        return;
    }

    debug!(
        container = container.name(),
        "Executing post-check command."
    );
    if client
        .execute_command(container.id(), &command, CHECK_TIMEOUT_MINUTES)
        .is_err()
    {
        error!(container = container.name(), "post-check command failed");
    }
}

/// ExecutePreUpdateCommand tries to run the pre-update lifecycle hook for a
/// single container. Returns a bool indicating whether to skip the update.
pub fn execute_pre_update_command<C: LifecycleClient>(
    client: &C,
    container: &Container,
) -> std::result::Result<bool, C::Error> {
    let timeout = container.pre_update_timeout();
    let command = container.get_lifecycle_pre_update_command();

    if command.is_empty() {
        debug!(
            container = container.name(),
            "No pre-update command supplied. Skipping"
        );
        return Ok(false);
    }

    if !container.is_running() || container.is_restarting() {
        debug!(
            container = container.name(),
            "Container is not running. Skipping pre-update command."
        );
        return Ok(false);
    }

    debug!(
        container = container.name(),
        "Executing pre-update command."
    );
    client.execute_command(container.id(), &command, timeout)
}

/// ExecutePostUpdateCommand tries to run the post-update lifecycle hook for a
/// single container.
pub fn execute_post_update_command<C: LifecycleClient>(client: &C, new_container_id: &ContainerID) {
    let new_container = match client.get_container(new_container_id) {
        Ok(container) => container,
        Err(_) => {
            error!(
                container_id = %new_container_id.short_id(),
                "post-update container lookup failed"
            );
            return;
        }
    };
    let timeout = new_container.post_update_timeout();

    let command = new_container.get_lifecycle_post_update_command();
    if command.is_empty() {
        debug!(
            container = new_container.name(),
            "No post-update command supplied. Skipping"
        );
        return;
    }

    debug!(
        container = new_container.name(),
        "Executing post-update command."
    );
    if client
        .execute_command(new_container_id, &command, timeout)
        .is_err()
    {
        error!(
            container = new_container.name(),
            "post-update command failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, VecDeque};

    use crate::container::{ContainerConfig, ContainerInspect, ContainerState};
    use crate::types::ImageID;

    const PRE_CHECK_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-check";
    // Reserved for parity with the legacy label set; kept for future post-check
    // coverage and to preserve the established label namespace.
    #[allow(dead_code)]
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
        let client =
            MockClient::new(Ok(vec![matched.clone(), skipped])).with_exec_results([Ok(false)]);
        let params = UpdateParams::new().with_filter(|container| container.name() == "alpha");

        execute_pre_checks(&client, &params).expect("pre-checks should never fail");

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
    fn execute_post_checks_swallows_list_errors() {
        let client = MockClient::new(Err(MockError::ListFailed));

        execute_post_checks(&client, &UpdateParams::new())
            .expect("post-check list errors are swallowed like the Go code");

        assert!(client.exec_calls().is_empty());
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
            false
        );
        assert_eq!(
            execute_pre_update_command(&client, &stopped).expect("stopped container is a no-op"),
            false
        );
        assert_eq!(
            execute_pre_update_command(&client, &restarting)
                .expect("restarting container is a no-op"),
            false
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

        let skip_update =
            execute_pre_update_command(&client, &container).expect("pre-update should execute");

        assert_eq!(skip_update, true);
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
            .with_get_result(&container_id, Ok(container))
            .with_exec_results([Ok(false)]);

        execute_post_update_command(&client, &container_id);

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
    fn execute_post_update_command_swallows_lookup_and_command_errors() {
        let missing_container_id = ContainerID::new("container-missing");
        let failing_container_id = ContainerID::new("container-post-update-error");
        let failing_container = lifecycle_container(
            failing_container_id.as_str(),
            "post-update-error",
            &[(POST_UPDATE_LABEL, "echo post-update")],
            true,
            false,
        );
        let client = MockClient::new(Ok(Vec::new()))
            .with_get_result(&failing_container_id, Ok(failing_container))
            .with_exec_results([Err(MockError::CommandFailed)]);

        execute_post_update_command(&client, &missing_container_id);
        execute_post_update_command(&client, &failing_container_id);

        assert_eq!(
            client.exec_calls(),
            vec![ExecCall {
                container_id: failing_container_id,
                command: "echo post-update".to_string(),
                timeout_minutes: CHECK_TIMEOUT_MINUTES,
            }]
        );
    }
}
