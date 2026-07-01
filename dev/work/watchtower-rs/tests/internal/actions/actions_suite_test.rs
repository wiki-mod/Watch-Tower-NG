#![forbid(unsafe_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;
use watchtower_rs::actions::{check_for_multiple_watchtower_instances, UpdateClient};
use watchtower_rs::container::{ContainerConfig, ContainerInspect, ContainerState, HostConfig, ImageInspect, Container};
use watchtower_rs::lifecycle::LifecycleClient;
use watchtower_rs::types::{ContainerID, ImageID, UpdateParams};
use std::collections::BTreeMap;

/// Mock client that tracks which containers were stopped and which images were removed
#[derive(Clone)]
struct TestClient {
    stopped_ids: Arc<Mutex<Vec<ContainerID>>>,
    removed_image_ids: Arc<Mutex<Vec<ImageID>>>,
}

impl TestClient {
    fn new() -> Self {
        Self {
            stopped_ids: Arc::new(Mutex::new(Vec::new())),
            removed_image_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn stopped_ids(&self) -> Vec<ContainerID> {
        self.stopped_ids.lock().unwrap().clone()
    }

    fn removed_image_ids(&self) -> Vec<ImageID> {
        self.removed_image_ids.lock().unwrap().clone()
    }
}

impl LifecycleClient for TestClient {
    type Error = String;

    fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error> {
        Ok(Vec::new())
    }

    fn get_container(
        &self,
        _container_id: &ContainerID,
    ) -> std::result::Result<Container, Self::Error> {
        Err("not implemented".to_string())
    }

    fn execute_command(
        &self,
        _container_id: &ContainerID,
        _command: &str,
        _timeout_minutes: i64,
    ) -> std::result::Result<bool, Self::Error> {
        Err("not implemented".to_string())
    }
}

impl UpdateClient for TestClient {
    fn is_container_stale(
        &self,
        _container: &Container,
        _params: &UpdateParams,
    ) -> std::result::Result<(bool, ImageID), Self::Error> {
        Err("not implemented".to_string())
    }

    fn stop_container(
        &self,
        container: &Container,
        _timeout: Duration,
    ) -> std::result::Result<(), Self::Error> {
        self.stopped_ids.lock().unwrap().push(container.id().clone());
        Ok(())
    }

    fn start_container(
        &self,
        _container: &Container,
    ) -> std::result::Result<ContainerID, Self::Error> {
        Err("not implemented".to_string())
    }

    fn rename_container(
        &self,
        _container: &Container,
        _new_name: &str,
    ) -> std::result::Result<(), Self::Error> {
        Err("not implemented".to_string())
    }

    fn remove_image_by_id(&self, image_id: &ImageID) -> std::result::Result<(), Self::Error> {
        self.removed_image_ids.lock().unwrap().push(image_id.clone());
        Ok(())
    }
}

fn test_container(id: &str, name: &str, image_id: &str, created_at_str: &str) -> Container {
    Container::new(
        ContainerInspect {
            id: ContainerID::new(id),
            name: format!("/{}", name),
            image: ImageID::new(image_id),
            created: created_at_str.to_string(),
            state: ContainerState {
                running: true,
                restarting: false,
            },
            config: Some(ContainerConfig {
                image: "test:latest".to_string(),
                labels: BTreeMap::new(),
                working_dir: String::new(),
                user: String::new(),
                entrypoint: Vec::new(),
                cmd: Vec::new(),
                env: Vec::new(),
                volumes: Default::default(),
                exposed_ports: None,
                healthcheck: None,
                hostname: String::new(),
            }),
            host_config: Some(HostConfig {
                links: Vec::new(),
                network_mode: Default::default(),
                port_bindings: Default::default(),
                auto_remove: false,
            }),
            network_settings: None,
        },
        Some(ImageInspect {
            id: ImageID::new(image_id),
            config: ContainerConfig {
                image: "test:latest".to_string(),
                labels: BTreeMap::new(),
                working_dir: String::new(),
                user: String::new(),
                entrypoint: Vec::new(),
                cmd: Vec::new(),
                env: Vec::new(),
                volumes: Default::default(),
                exposed_ports: None,
                healthcheck: None,
                hostname: String::new(),
            },
        }),
    )
}

#[test]
fn actions_suite_empty_slice_is_a_no_op() {
    let client = TestClient::new();
    let containers: Vec<Container> = Vec::new();

    assert!(check_for_multiple_watchtower_instances(&client, &containers, false).is_ok());
    assert_eq!(client.stopped_ids().len(), 0);
}

#[test]
fn actions_suite_single_container_is_a_no_op() {
    let client = TestClient::new();
    let containers = vec![test_container(
        "test-container",
        "test-container",
        "watchtower",
        "2024-06-18T12:00:00Z",
    )];

    assert!(check_for_multiple_watchtower_instances(&client, &containers, false).is_ok());
    assert_eq!(client.stopped_ids().len(), 0);
}

#[test]
fn actions_suite_multiple_containers_keep_the_latest_instance() {
    let client = TestClient::new();
    let containers = vec![
        test_container(
            "test-container-01",
            "test-container-01",
            "watchtower",
            "2024-06-17T12:00:00Z",
        ),
        test_container(
            "test-container-02",
            "test-container-02",
            "watchtower",
            "2024-06-18T12:00:00Z",
        ),
    ];

    assert!(check_for_multiple_watchtower_instances(&client, &containers, false).is_ok());

    let stopped = client.stopped_ids();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0], ContainerID::from("test-container-01"));
    assert_eq!(client.removed_image_ids().len(), 0);
}

#[test]
fn actions_suite_cleanup_flag_requests_image_cleanup() {
    let client = TestClient::new();
    let containers = vec![
        test_container(
            "test-container-01",
            "test-container-01",
            "watchtower",
            "2024-06-17T12:00:00Z",
        ),
        test_container(
            "test-container-02",
            "test-container-02",
            "watchtower",
            "2024-06-18T12:00:00Z",
        ),
    ];

    assert!(check_for_multiple_watchtower_instances(&client, &containers, true).is_ok());

    let stopped = client.stopped_ids();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0], ContainerID::from("test-container-01"));

    let removed = client.removed_image_ids();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0], ImageID::from("watchtower"));
}

#[test]
fn actions_suite_cleanup_flag_disabled_skips_image_cleanup() {
    let client = TestClient::new();
    let containers = vec![
        test_container(
            "test-container-01",
            "test-container-01",
            "watchtower",
            "2024-06-17T12:00:00Z",
        ),
        test_container(
            "test-container-02",
            "test-container-02",
            "watchtower",
            "2024-06-18T12:00:00Z",
        ),
    ];

    assert!(check_for_multiple_watchtower_instances(&client, &containers, false).is_ok());

    let stopped = client.stopped_ids();
    assert_eq!(stopped.len(), 1);
    assert_eq!(stopped[0], ContainerID::from("test-container-01"));
    assert_eq!(client.removed_image_ids().len(), 0);
}
