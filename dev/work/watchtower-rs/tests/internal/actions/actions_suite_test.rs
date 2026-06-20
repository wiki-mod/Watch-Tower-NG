#![forbid(unsafe_code)]

use watchtower_rs::actions::check_for_multiple_watchtower_instances;
use watchtower_rs::types::{ContainerID, ImageID, RuntimeContainer, UpdateParams};

#[derive(Clone)]
struct MockContainer {
    id: ContainerID,
    name: String,
    links: Vec<String>,
    image_id: ImageID,
    created_at: String,
    stale: bool,
    linked_to_restarting: bool,
    monitor_only: bool,
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
        &self.created_at
    }

    fn is_watchtower(&self) -> bool {
        true
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

fn container(id: &str, name: &str, image_id: &str, created_at: &str) -> MockContainer {
    MockContainer {
        id: ContainerID::from(id),
        name: name.to_string(),
        links: Vec::new(),
        image_id: ImageID::from(image_id),
        created_at: created_at.to_string(),
        stale: false,
        linked_to_restarting: false,
        monitor_only: false,
    }
}

#[test]
fn actions_suite_empty_slice_is_a_no_op() {
    let containers: Vec<MockContainer> = Vec::new();

    assert_eq!(check_for_multiple_watchtower_instances(&containers, false), None);
}

#[test]
fn actions_suite_single_container_is_a_no_op() {
    let containers = vec![container(
        "test-container",
        "test-container",
        "watchtower",
        "2024-06-18T12:00:00Z",
    )];

    assert_eq!(check_for_multiple_watchtower_instances(&containers, false), None);
}

#[test]
fn actions_suite_multiple_containers_keep_the_latest_instance() {
    let containers = vec![
        container(
            "test-container-01",
            "test-container-01",
            "watchtower",
            "2024-06-17T12:00:00Z",
        ),
        container(
            "test-container-02",
            "test-container-02",
            "watchtower",
            "2024-06-18T12:00:00Z",
        ),
    ];

    let plan = check_for_multiple_watchtower_instances(&containers, false)
        .expect("cleanup plan should exist");

    assert_eq!(
        plan.stop_container_ids,
        vec![ContainerID::from("test-container-01")]
    );
    assert!(plan.cleanup_image_ids.is_empty());
}

#[test]
fn actions_suite_cleanup_flag_requests_image_cleanup() {
    let containers = vec![
        container(
            "test-container-01",
            "test-container-01",
            "watchtower",
            "2024-06-17T12:00:00Z",
        ),
        container(
            "test-container-02",
            "test-container-02",
            "watchtower",
            "2024-06-18T12:00:00Z",
        ),
    ];

    let plan = check_for_multiple_watchtower_instances(&containers, true)
        .expect("cleanup plan should exist");

    assert_eq!(
        plan.stop_container_ids,
        vec![ContainerID::from("test-container-01")]
    );
    assert_eq!(plan.cleanup_image_ids, vec![ImageID::from("watchtower")]);
}

#[test]
fn actions_suite_cleanup_flag_disabled_skips_image_cleanup() {
    let containers = vec![
        container(
            "test-container-01",
            "test-container-01",
            "watchtower",
            "2024-06-17T12:00:00Z",
        ),
        container(
            "test-container-02",
            "test-container-02",
            "watchtower",
            "2024-06-18T12:00:00Z",
        ),
    ];

    let plan = check_for_multiple_watchtower_instances(&containers, false)
        .expect("cleanup plan should exist");

    assert_eq!(
        plan.stop_container_ids,
        vec![ContainerID::from("test-container-01")]
    );
    assert!(plan.cleanup_image_ids.is_empty());
}
