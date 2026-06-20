#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use watchtower_rs::container::{
    Container, ContainerConfig, ContainerInspect, ContainerState, HealthConfig, HostConfig,
    ImageInspect, NetworkMode, PortBinding,
};
use watchtower_rs::docker_client::NetworkEndpoint;
use watchtower_rs::types::{ImageID, UpdateParams};

const CONTAINER_ID: &str = "container_id";
const CONTAINER_NAME: &str = "test-containrrr";
const CURRENT_IMAGE_ID: &str = "sha256:current";
const IMAGE_ID: &str = "sha256:image";
const CREATED_AT: &str = "2024-06-18T12:00:00Z";

fn labels(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

fn container(
    config: Option<ContainerConfig>,
    host_config: Option<HostConfig>,
    network_settings: Option<HashMap<String, NetworkEndpoint>>,
    image_config: Option<ContainerConfig>,
) -> Container {
    let inspect = ContainerInspect {
        id: CONTAINER_ID.into(),
        name: CONTAINER_NAME.to_string(),
        image: CURRENT_IMAGE_ID.into(),
        created: CREATED_AT.to_string(),
        state: ContainerState {
            running: true,
            restarting: false,
        },
        config,
        host_config,
        network_settings,
    };
    let image_info = image_config.map(|config| ImageInspect {
        id: ImageID::from(IMAGE_ID),
        config,
    });

    Container::new(inspect, image_info)
}

fn host_config(links: Vec<String>, port_bindings: BTreeMap<String, Vec<PortBinding>>) -> HostConfig {
    HostConfig {
        links,
        network_mode: NetworkMode::Default,
        port_bindings,
        auto_remove: false,
    }
}

fn container_config(
    image: &str,
    labels: BTreeMap<String, String>,
    exposed_ports: Option<BTreeSet<String>>,
    healthcheck: Option<HealthConfig>,
) -> ContainerConfig {
    ContainerConfig {
        image: image.to_string(),
        labels,
        working_dir: String::new(),
        user: String::new(),
        entrypoint: Vec::new(),
        cmd: Vec::new(),
        env: Vec::new(),
        volumes: BTreeSet::new(),
        exposed_ports,
        healthcheck,
        hostname: String::new(),
    }
}

fn default_container_config() -> ContainerConfig {
    container_config("image-name", BTreeMap::new(), None, None)
}

#[test]
fn verify_configuration_returns_an_error_when_image_info_is_missing() {
    let mut c = container(
        Some(ContainerConfig {
            exposed_ports: Some(BTreeSet::from(["80/tcp".to_string()])),
            ..default_container_config()
        }),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        None,
    );

    let err = c.verify_configuration().expect_err("expected an error");
    assert_eq!(err.to_string(), "invalid config: no available image info");
}

#[test]
fn verify_configuration_returns_an_error_when_config_is_missing() {
    let mut c = container(None, Some(host_config(Vec::new(), BTreeMap::new())), None, Some(default_container_config()));

    let err = c.verify_configuration().expect_err("expected an error");
    assert_eq!(
        err.to_string(),
        "invalid config: container configuration missing or invalid"
    );
}

#[test]
fn verify_configuration_returns_an_error_when_host_config_is_missing() {
    let mut c = container(
        Some(default_container_config()),
        None,
        None,
        Some(default_container_config()),
    );

    let err = c.verify_configuration().expect_err("expected an error");
    assert_eq!(
        err.to_string(),
        "invalid config: container configuration missing or invalid"
    );
}

#[test]
fn verify_configuration_accepts_missing_port_bindings() {
    let mut c = container(
        Some(default_container_config()),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    c.verify_configuration().expect("expected verification to succeed");
    let exposed_ports = c
        .container_info()
        .and_then(|info| info.config.as_ref())
        .and_then(|config| config.exposed_ports.as_ref());
    assert!(exposed_ports.is_none());
}

#[test]
fn verify_configuration_repairs_missing_exposed_ports() {
    let mut c = container(
        Some(ContainerConfig {
            exposed_ports: None,
            ..default_container_config()
        }),
        Some(host_config(
            Vec::new(),
            BTreeMap::from([("80/tcp".to_string(), vec![PortBinding::default()])]),
        )),
        None,
        Some(default_container_config()),
    );

    c.verify_configuration().expect("expected verification to succeed");

    let exposed_ports = c
        .container_info()
        .and_then(|info| info.config.as_ref())
        .and_then(|config| config.exposed_ports.as_ref())
        .expect("expected exposed ports to be initialized");
    assert!(exposed_ports.is_empty());
}

#[test]
fn verify_configuration_accepts_non_nil_exposed_ports_with_port_bindings() {
    let mut c = container(
        Some(ContainerConfig {
            exposed_ports: Some(BTreeSet::from(["80/tcp".to_string()])),
            ..default_container_config()
        }),
        Some(host_config(
            Vec::new(),
            BTreeMap::from([("80/tcp".to_string(), vec![PortBinding::default()])]),
        )),
        None,
        Some(default_container_config()),
    );

    c.verify_configuration().expect("expected verification to succeed");
    let exposed_ports = c
        .container_info()
        .and_then(|info| info.config.as_ref())
        .and_then(|config| config.exposed_ports.as_ref())
        .expect("expected exposed ports to remain present");
    assert_eq!(exposed_ports, &BTreeSet::from(["80/tcp".to_string()]));
}

#[test]
fn get_create_config_clears_equal_healthcheck_values() {
    let cases = [
        (
            HealthConfig {
                test: vec!["/usr/bin/sleep".to_string(), "1s".to_string()],
                ..HealthConfig::default()
            },
            HealthConfig {
                test: vec!["/usr/bin/sleep".to_string(), "1s".to_string()],
                ..HealthConfig::default()
            },
        ),
        (
            HealthConfig {
                timeout: Duration::from_secs(30),
                ..HealthConfig::default()
            },
            HealthConfig {
                timeout: Duration::from_secs(30),
                ..HealthConfig::default()
            },
        ),
        (
            HealthConfig {
                start_period: Duration::from_secs(30),
                ..HealthConfig::default()
            },
            HealthConfig {
                start_period: Duration::from_secs(30),
                ..HealthConfig::default()
            },
        ),
        (
            HealthConfig {
                retries: 30,
                ..HealthConfig::default()
            },
            HealthConfig {
                retries: 30,
                ..HealthConfig::default()
            },
        ),
    ];

    for (container_healthcheck, image_healthcheck) in cases {
        let mut c = container(
            Some(container_config(
                "image-name",
                BTreeMap::new(),
                None,
                Some(container_healthcheck),
            )),
            Some(host_config(Vec::new(), BTreeMap::new())),
            None,
            Some(container_config(
                "image-name:latest",
                BTreeMap::new(),
                None,
                Some(image_healthcheck),
            )),
        );

        assert_eq!(c.get_create_config().expect("create config").healthcheck, Some(HealthConfig::default()));
    }
}

#[test]
fn get_create_config_returns_the_container_healthcheck_when_it_differs_from_the_image() {
    let mut c = container(
        Some(container_config(
            "image-name",
            BTreeMap::new(),
            None,
            Some(HealthConfig {
                test: vec!["/usr/bin/sleep".to_string(), "1s".to_string()],
                interval: Duration::from_secs(30),
                timeout: Duration::from_secs(30),
                start_period: Duration::from_secs(10),
                retries: 2,
            }),
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(container_config(
            "image-name:latest",
            BTreeMap::new(),
            None,
            Some(HealthConfig {
                test: vec!["/usr/bin/sleep".to_string(), "10s".to_string()],
                interval: Duration::from_secs(10),
                timeout: Duration::from_secs(60),
                start_period: Duration::from_secs(30),
                retries: 10,
            }),
        )),
    );

    assert_eq!(
        c.get_create_config().expect("create config").healthcheck,
        Some(HealthConfig {
            test: vec!["/usr/bin/sleep".to_string(), "1s".to_string()],
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(30),
            start_period: Duration::from_secs(10),
            retries: 2,
        })
    );
}

#[test]
fn get_create_config_leaves_healthcheck_empty_when_container_healthcheck_is_missing() {
    let mut c = container(
        Some(container_config("image-name", BTreeMap::new(), None, None)),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(container_config(
            "image-name:latest",
            BTreeMap::new(),
            None,
            Some(HealthConfig {
                test: vec!["/usr/bin/sleep".to_string(), "10s".to_string()],
                interval: Duration::from_secs(10),
                timeout: Duration::from_secs(60),
                start_period: Duration::from_secs(30),
                retries: 10,
            }),
        )),
    );

    assert_eq!(c.get_create_config().expect("create config").healthcheck, None);
}

#[test]
fn get_create_config_keeps_the_container_healthcheck_when_the_image_has_none() {
    let mut c = container(
        Some(container_config(
            "image-name",
            BTreeMap::new(),
            None,
            Some(HealthConfig {
                test: vec!["/usr/bin/sleep".to_string(), "1s".to_string()],
                interval: Duration::from_secs(30),
                timeout: Duration::from_secs(30),
                start_period: Duration::from_secs(10),
                retries: 2,
            }),
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(container_config("image-name:latest", BTreeMap::new(), None, None)),
    );

    assert_eq!(
        c.get_create_config().expect("create config").healthcheck,
        Some(HealthConfig {
            test: vec!["/usr/bin/sleep".to_string(), "1s".to_string()],
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(30),
            start_period: Duration::from_secs(10),
            retries: 2,
        })
    );
}

#[test]
fn name_returns_the_container_name() {
    let c = container(
        Some(default_container_config()),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    let name = c.name();
    assert_eq!(name, CONTAINER_NAME);
    assert_ne!(name, "wrong-name");
}

#[test]
fn id_returns_the_container_id() {
    let c = container(
        Some(default_container_config()),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    let id = c.id();
    assert_eq!(id.as_str(), CONTAINER_ID);
    assert_ne!(id.as_str(), "wrong-id");
}

#[test]
fn enabled_returns_true_and_true_if_enabled() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.enable", "true")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.enabled(), (true, true));
}

#[test]
fn enabled_returns_false_and_true_if_present_but_not_true() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.enable", "false")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.enabled(), (false, true));
}

#[test]
fn enabled_returns_false_and_false_if_not_present() {
    let c = container(
        Some(container_config("image-name", labels(&[("lol", "false")]), None, None)),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.enabled(), (false, false));
}

#[test]
fn enabled_returns_false_and_false_if_present_but_not_parsable() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.enable", "falsy")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.enabled(), (false, false));
}

#[test]
fn is_watchtower_returns_true_only_when_the_label_is_true() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower", "true")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert!(c.is_watchtower());

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower", "false")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(!c.is_watchtower());

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("funny.label", "false")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(!c.is_watchtower());

    let c = container(
        Some(container_config("image-name", BTreeMap::new(), None, None)),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(!c.is_watchtower());
}

#[test]
fn stop_signal_returns_the_label_value_or_empty_string() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.stop-signal", "SIGKILL")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert_eq!(c.stop_signal(), "SIGKILL");

    let c = container(
        Some(container_config("image-name", BTreeMap::new(), None, None)),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert_eq!(c.stop_signal(), "");
}

#[test]
fn image_name_uses_the_zodiac_label_when_present() {
    let c = container(
        Some(container_config(
            "ignored",
            labels(&[("com.centurylinklabs.zodiac.original-image", "the-original-image")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.image_name(), "the-original-image:latest");
}

#[test]
fn image_name_returns_the_image_name() {
    let c = container(
        Some(container_config("image-name:3", BTreeMap::new(), None, None)),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.image_name(), "image-name:3");
}

#[test]
fn image_name_assumes_latest_when_no_tag_is_supplied() {
    let c = container(
        Some(container_config("image-name", BTreeMap::new(), None, None)),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.image_name(), "image-name:latest");
}

#[test]
fn links_are_derived_from_the_depends_on_label() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.depends-on", "postgres")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert_eq!(c.links(), &["/postgres".to_string()]);

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.depends-on", "postgres,redis")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert_eq!(
        c.links(),
        &["/postgres".to_string(), "/redis".to_string()]
    );

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.depends-on", "/postgres,redis")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert_eq!(c.links(), &["/postgres".to_string(), "/redis".to_string()]);

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.depends-on", "")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(c.links().is_empty());
}

#[test]
fn links_fall_back_to_host_config_links_when_the_label_is_missing() {
    let c = container(
        Some(default_container_config()),
        Some(host_config(
            vec![
                "redis:test-containrrr".to_string(),
                "postgres:test-containrrr".to_string(),
            ],
            BTreeMap::new(),
        )),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.links(), &["redis".to_string(), "postgres".to_string()]);
}

#[test]
fn is_no_pull_obeys_label_and_global_settings() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.no-pull", "true")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(c.is_no_pull(&UpdateParams::default()));

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.no-pull", "false")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(!c.is_no_pull(&UpdateParams::default()));

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.no-pull", "maybe")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(!c.is_no_pull(&UpdateParams::default()));

    let c = container(
        Some(container_config("image-name", BTreeMap::new(), None, None)),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(!c.is_no_pull(&UpdateParams::default()));

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.no-pull", "true")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(c.is_no_pull(&UpdateParams {
        no_pull: true,
        ..UpdateParams::default()
    }));

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.no-pull", "false")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(c.is_no_pull(&UpdateParams {
        no_pull: true,
        ..UpdateParams::default()
    }));

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.no-pull", "true")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(c.is_no_pull(&UpdateParams {
        no_pull: true,
        label_precedence: true,
        ..UpdateParams::default()
    }));

    let c = container(
        Some(container_config(
            "image-name",
            labels(&[("com.centurylinklabs.watchtower.no-pull", "false")]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );
    assert!(!c.is_no_pull(&UpdateParams {
        no_pull: true,
        label_precedence: true,
        ..UpdateParams::default()
    }));
}

#[test]
fn pre_and_post_update_timeouts_are_returned_in_minutes() {
    let c = container(
        Some(container_config(
            "image-name",
            labels(&[
                ("com.centurylinklabs.watchtower.lifecycle.pre-update-timeout", "3"),
                ("com.centurylinklabs.watchtower.lifecycle.post-update-timeout", "5"),
            ]),
            None,
            None,
        )),
        Some(host_config(Vec::new(), BTreeMap::new())),
        None,
        Some(default_container_config()),
    );

    assert_eq!(c.pre_update_timeout(), 3);
    assert_eq!(c.post_update_timeout(), 5);
}
