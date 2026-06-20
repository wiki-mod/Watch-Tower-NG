#![forbid(unsafe_code)]

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use watchtower_rs::actions::{update, UpdateClient};
use watchtower_rs::container::{
    Container, ContainerConfig, ContainerInspect, ContainerState, HostConfig, ImageInspect,
};
use watchtower_rs::lifecycle::LifecycleClient;
use watchtower_rs::types::{ContainerID, ImageID, UpdateParams};

const CREATED_AT: &str = "2024-06-18T12:00:00Z";
const MONITOR_ONLY_LABEL: &str = "com.centurylinklabs.watchtower.monitor-only";
const DEPENDS_ON_LABEL: &str = "com.centurylinklabs.watchtower.depends-on";
const PRE_UPDATE_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-update";
const PRE_UPDATE_TIMEOUT_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-update-timeout";

#[derive(Clone)]
struct TestData {
    tried_to_remove_image_count: Cell<usize>,
    name_of_container_to_keep: String,
    containers: Vec<Container>,
    staleness: BTreeMap<String, bool>,
}

impl TestData {
    fn new(name_of_container_to_keep: impl Into<String>, containers: Vec<Container>) -> Self {
        Self {
            tried_to_remove_image_count: Cell::new(0),
            name_of_container_to_keep: name_of_container_to_keep.into(),
            containers,
            staleness: BTreeMap::new(),
        }
    }
}

struct MockUpdateClient {
    test_data: TestData,
}

impl MockUpdateClient {
    fn new(test_data: TestData) -> Self {
        Self { test_data }
    }
}

fn create_mock_client(test_data: TestData) -> MockUpdateClient {
    MockUpdateClient::new(test_data)
}

fn labels(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn mock_image_info(image: &str) -> ImageInspect {
    ImageInspect {
        id: ImageID::from(image),
        config: ContainerConfig::default(),
    }
}

fn mock_container_with_config(
    id: &str,
    name: &str,
    image: &str,
    running: bool,
    restarting: bool,
    labels: BTreeMap<String, String>,
    image_info: Option<ImageInspect>,
) -> Container {
    Container::new(
        ContainerInspect {
            id: ContainerID::from(id),
            name: name.to_string(),
            image: ImageID::from(image),
            created: CREATED_AT.to_string(),
            state: ContainerState { running, restarting },
            config: Some(ContainerConfig {
                image: image.to_string(),
                labels,
                exposed_ports: Some(BTreeSet::new()),
                ..ContainerConfig::default()
            }),
            host_config: Some(HostConfig::default()),
            network_settings: None,
        },
        image_info,
    )
}

fn mock_container(id: &str, name: &str, image: &str, created: &str) -> Container {
    Container::new(
        ContainerInspect {
            id: ContainerID::from(id),
            name: name.to_string(),
            image: ImageID::from(image),
            created: created.to_string(),
            state: ContainerState {
                running: true,
                restarting: false,
            },
            config: Some(ContainerConfig {
                image: image.to_string(),
                labels: BTreeMap::new(),
                exposed_ports: Some(BTreeSet::new()),
                ..ContainerConfig::default()
            }),
            host_config: Some(HostConfig::default()),
            network_settings: None,
        },
        Some(mock_image_info(image)),
    )
}

fn common_test_data(name_of_container_to_keep: &str) -> TestData {
    TestData::new(
        name_of_container_to_keep,
        vec![
            mock_container(
                "test-container-01",
                "test-container-01",
                "fake-image:latest",
                "2024-06-17T12:00:00Z",
            ),
            mock_container(
                "test-container-02",
                "test-container-02",
                "fake-image:latest",
                "2024-06-18T12:00:00Z",
            ),
            mock_container(
                "test-container-02",
                "test-container-02",
                "fake-image:latest",
                "2024-06-18T12:00:00Z",
            ),
        ],
    )
}

fn linked_test_data(with_image_info: bool) -> TestData {
    let stale_container = mock_container(
        "test-container-01",
        "/test-container-01",
        "fake-image1:latest",
        "2024-06-17T12:00:00Z",
    );

    let image_info = with_image_info.then(|| mock_image_info("test-container-02"));
    let linking_container = mock_container_with_config(
        "test-container-02",
        "/test-container-02",
        "fake-image2:latest",
        true,
        false,
        BTreeMap::new(),
        image_info,
    );

    let mut staleness = BTreeMap::new();
    staleness.insert(linking_container.name().to_string(), false);

    let mut test_data = TestData::new("", vec![stale_container, linking_container]);
    test_data.staleness = staleness;
    test_data
}

impl LifecycleClient for MockUpdateClient {
    type Error = String;

    fn list_containers(&self) -> std::result::Result<Vec<Container>, Self::Error> {
        Ok(self.test_data.containers.clone())
    }

    fn get_container(
        &self,
        _container_id: &ContainerID,
    ) -> std::result::Result<Container, Self::Error> {
        self.test_data
            .containers
            .first()
            .cloned()
            .ok_or_else(|| "not used".to_string())
    }

    fn execute_command(
        &self,
        _container_id: &ContainerID,
        command: &str,
        _timeout_minutes: i64,
    ) -> std::result::Result<bool, Self::Error> {
        match command {
            "/PreUpdateReturn0.sh" => Ok(false),
            "/PreUpdateReturn1.sh" => Err("command exited with code 1".to_string()),
            "/PreUpdateReturn75.sh" => Ok(true),
            _ => Ok(false),
        }
    }
}

impl UpdateClient for MockUpdateClient {
    fn is_container_stale(
        &self,
        container: &Container,
        _params: &UpdateParams,
    ) -> std::result::Result<(bool, ImageID), Self::Error> {
        let stale = self
            .test_data
            .staleness
            .get(container.name())
            .copied()
            .unwrap_or(true);

        if stale {
            Ok((true, ImageID::new("")))
        } else {
            Ok((false, container.image_id().clone()))
        }
    }

    fn stop_container(
        &self,
        container: &Container,
        _timeout: std::time::Duration,
    ) -> std::result::Result<(), Self::Error> {
        if container.name() == self.test_data.name_of_container_to_keep {
            return Err("tried to stop the instance we want to keep".to_string());
        }

        Ok(())
    }

    fn start_container(
        &self,
        container: &Container,
    ) -> std::result::Result<ContainerID, Self::Error> {
        Ok(container.id().clone())
    }

    fn rename_container(
        &self,
        _container: &Container,
        _new_name: &str,
    ) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    fn remove_image_by_id(&self, _image_id: &ImageID) -> std::result::Result<(), Self::Error> {
        self.test_data
            .tried_to_remove_image_count
            .set(self.test_data.tried_to_remove_image_count.get() + 1);
        Ok(())
    }
}

#[test]
fn cleanup_multiple_containers_using_same_image_removes_it_once() {
    let client = create_mock_client(common_test_data(""));

    let params = UpdateParams {
        cleanup: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}

#[test]
fn cleanup_multiple_containers_using_different_images_removes_each_one() {
    let mut test_data = common_test_data("");
    test_data.containers.push(mock_container(
        "unique-test-container",
        "unique-test-container",
        "unique-fake-image:latest",
        "2024-06-18T12:00:00Z",
    ));
    let client = create_mock_client(test_data);

    let params = UpdateParams {
        cleanup: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 2);
}

#[test]
fn cleanup_skips_images_for_linked_containers() {
    let client = create_mock_client(linked_test_data(true));

    let params = UpdateParams {
        cleanup: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}

#[test]
fn rolling_restart_removes_the_image_once() {
    let client = create_mock_client(common_test_data(""));

    let params = UpdateParams {
        cleanup: true,
        rolling_restart: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}

#[test]
fn linked_container_with_missing_image_info_fails_gracefully() {
    let client = create_mock_client(linked_test_data(false));

    let report = update(&client, &UpdateParams::default()).expect("update should succeed");

    assert_eq!(report.updated().len(), 1);
    assert_eq!(report.fresh().len(), 1);
}

#[test]
fn monitor_only_label_skips_the_marked_container() {
    let client = create_mock_client(TestData::new(
        "",
        vec![
            mock_container_with_config(
                "test-container-01",
                "test-container-01",
                "fake-image1:latest",
                true,
                false,
                BTreeMap::new(),
                Some(mock_image_info("fake-image1:latest")),
            ),
            mock_container_with_config(
                "test-container-02",
                "test-container-02",
                "fake-image2:latest",
                true,
                false,
                labels(&[(MONITOR_ONLY_LABEL, "true")]),
                Some(mock_image_info("fake-image2:latest")),
            ),
        ],
    ));

    let params = UpdateParams {
        cleanup: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}

#[test]
fn global_monitor_only_skips_every_container() {
    let client = create_mock_client(TestData::new(
        "",
        vec![
            mock_container(
                "test-container-01",
                "test-container-01",
                "fake-image:latest",
                "2024-06-18T12:00:00Z",
            ),
            mock_container(
                "test-container-02",
                "test-container-02",
                "fake-image:latest",
                "2024-06-18T12:00:00Z",
            ),
        ],
    ));

    let params = UpdateParams {
        cleanup: true,
        monitor_only: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 0);
}

#[test]
fn label_precedence_allows_monitor_only_false_to_update() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container_with_config(
            "test-container-02",
            "test-container-02",
            "fake-image2:latest",
            true,
            false,
            labels(&[(MONITOR_ONLY_LABEL, "false")]),
            Some(mock_image_info("fake-image2:latest")),
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        monitor_only: true,
        label_precedence: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}

#[test]
fn label_precedence_blocks_monitor_only_true() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container_with_config(
            "test-container-02",
            "test-container-02",
            "fake-image2:latest",
            true,
            false,
            labels(&[(MONITOR_ONLY_LABEL, "true")]),
            Some(mock_image_info("fake-image2:latest")),
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        monitor_only: true,
        label_precedence: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 0);
}

#[test]
fn label_precedence_skips_when_the_label_is_missing() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container(
            "test-container-01",
            "test-container-01",
            "fake-image:latest",
            "2024-06-18T12:00:00Z",
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        monitor_only: true,
        label_precedence: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 0);
}

#[test]
fn pre_update_script_returning_one_skips_the_update() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container_with_config(
            "test-container-02",
            "test-container-02",
            "fake-image2:latest",
            true,
            false,
            labels(&[
                (PRE_UPDATE_TIMEOUT_LABEL, "190"),
                (PRE_UPDATE_LABEL, "/PreUpdateReturn1.sh"),
            ]),
            Some(mock_image_info("fake-image2:latest")),
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        lifecycle_hooks: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 0);
}

#[test]
fn pre_update_script_returning_75_skips_the_update() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container_with_config(
            "test-container-02",
            "test-container-02",
            "fake-image2:latest",
            true,
            false,
            labels(&[
                (PRE_UPDATE_TIMEOUT_LABEL, "190"),
                (PRE_UPDATE_LABEL, "/PreUpdateReturn75.sh"),
            ]),
            Some(mock_image_info("fake-image2:latest")),
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        lifecycle_hooks: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 0);
}

#[test]
fn pre_update_script_returning_zero_updates_the_container() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container_with_config(
            "test-container-02",
            "test-container-02",
            "fake-image2:latest",
            true,
            false,
            labels(&[
                (PRE_UPDATE_TIMEOUT_LABEL, "190"),
                (PRE_UPDATE_LABEL, "/PreUpdateReturn0.sh"),
            ]),
            Some(mock_image_info("fake-image2:latest")),
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        lifecycle_hooks: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}

#[test]
fn update_marks_dependents_for_restart() {
    let mut provider = mock_container_with_config(
        "test-container-provider",
        "/test-container-provider",
        "fake-image2:latest",
        true,
        false,
        BTreeMap::new(),
        Some(mock_image_info("fake-image2:latest")),
    );
    provider.set_stale(true);

    let consumer = mock_container_with_config(
        "test-container-consumer",
        "/test-container-consumer",
        "fake-image3:latest",
        true,
        false,
        labels(&[(DEPENDS_ON_LABEL, "/test-container-provider")]),
        Some(mock_image_info("fake-image3:latest")),
    );

    let client = create_mock_client(TestData::new("", vec![provider, consumer]));
    let params = UpdateParams::default();

    let report = update(&client, &params).expect("update should succeed");

    assert_eq!(report.updated().len(), 2);
}

#[test]
fn pre_update_skips_when_container_is_not_running() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container_with_config(
            "test-container-02",
            "test-container-02",
            "fake-image2:latest",
            false,
            false,
            labels(&[
                (PRE_UPDATE_TIMEOUT_LABEL, "190"),
                (PRE_UPDATE_LABEL, "/PreUpdateReturn1.sh"),
            ]),
            Some(mock_image_info("fake-image2:latest")),
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        lifecycle_hooks: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}

#[test]
fn pre_update_skips_when_container_is_restarting() {
    let client = create_mock_client(TestData::new(
        "",
        vec![mock_container_with_config(
            "test-container-02",
            "test-container-02",
            "fake-image2:latest",
            false,
            true,
            labels(&[
                (PRE_UPDATE_TIMEOUT_LABEL, "190"),
                (PRE_UPDATE_LABEL, "/PreUpdateReturn1.sh"),
            ]),
            Some(mock_image_info("fake-image2:latest")),
        )],
    ));

    let params = UpdateParams {
        cleanup: true,
        lifecycle_hooks: true,
        ..UpdateParams::default()
    };

    let _ = update(&client, &params).expect("update should succeed");

    assert_eq!(client.test_data.tried_to_remove_image_count.get(), 1);
}
