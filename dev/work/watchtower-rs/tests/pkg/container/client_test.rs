#![forbid(unsafe_code)]

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use watchtower_rs::actions::UpdateClient;
use watchtower_rs::container::{
    Container, ContainerConfig, ContainerInspect, ContainerState, HostConfig, NetworkMode,
};
use watchtower_rs::docker_client::{
    container_list_statuses, normalize_network_config, simple_network_config,
    warn_on_head_pull_failed, warn_on_head_pull_failed_for_container, DockerCliAdapter,
    DockerCliError, NetworkEndpoint, NetworkingConfig, WarningStrategy,
};
use watchtower_rs::filters::{self, FilterableContainer as LegacyFilterableContainer};
use watchtower_rs::lifecycle::LifecycleClient;
use watchtower_rs::types::{ContainerID, ImageID, UpdateParams};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static TEMP_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

fn make_container(
    id: &str,
    name: &str,
    image_name: &str,
    image_id: &str,
    labels: &[(&str, &str)],
    running: bool,
    restarting: bool,
    network_mode: NetworkMode,
    network_settings: Option<HashMap<String, NetworkEndpoint>>,
) -> Container {
    let labels = labels
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<BTreeMap<_, _>>();

    Container::new(
        ContainerInspect {
            id: ContainerID::from(id),
            name: name.to_string(),
            image: ImageID::from(image_id),
            created: "2026-06-20T11:00:00Z".to_string(),
            state: ContainerState {
                running,
                restarting,
            },
            config: Some(ContainerConfig {
                image: image_name.to_string(),
                labels,
                hostname: "demo-host".to_string(),
                ..ContainerConfig::default()
            }),
            host_config: Some(HostConfig {
                network_mode,
                ..HostConfig::default()
            }),
            network_settings,
        },
        None,
    )
}

#[derive(Debug)]
struct FilterContainer {
    name: String,
    watchtower: bool,
    image_name: String,
}

impl LegacyFilterableContainer for FilterContainer {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_watchtower(&self) -> bool {
        self.watchtower
    }

    fn enabled(&self) -> (bool, bool) {
        (true, true)
    }

    fn scope(&self) -> (Option<&str>, bool) {
        (None, false)
    }

    fn image_name(&self) -> &str {
        self.image_name.as_str()
    }
}

fn names_matching<'a>(
    containers: &'a [FilterContainer],
    filter: filters::Filter<'a, FilterContainer>,
) -> Vec<String> {
    containers
        .iter()
        .filter(|container| filter(container))
        .map(|container| container.name().to_string())
        .collect()
}

fn container_with_aliases(image_name: &str, aliases: &[&str]) -> Container {
    let network_settings = Some(HashMap::from([(
        "test".to_string(),
        NetworkEndpoint {
            aliases: aliases.iter().map(|alias| (*alias).to_string()).collect(),
        },
    )]));

    make_container(
        "1234567890ab1234567890ab1234567890ab1234567890ab1234567890abcd",
        "/demo",
        image_name,
        "sha256:current",
        &[],
        true,
        false,
        NetworkMode::Default,
        network_settings,
    )
}

fn unique_temp_path(prefix: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be monotonic enough for tests")
        .as_nanos();
    let seq = TEMP_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{stamp}-{seq}", std::process::id()))
}

fn write_executable_script(script: &str) -> PathBuf {
    let dir = unique_temp_path("watchtower-rs-client-test");
    fs::create_dir_all(&dir).expect("temp dir should be creatable");

    let path = dir.join("docker");
    fs::write(&path, script).expect("script should be writable");

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&path)
            .expect("script metadata should be readable")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("script should be executable");
    }

    path
}

fn fake_docker_adapter(script: &str) -> DockerCliAdapter {
    let path = write_executable_script(script);
    DockerCliAdapter::with_binary(path.to_string_lossy().into_owned())
}

fn compile_helper_binary(source: &str) -> PathBuf {
    let dir = unique_temp_path("watchtower-rs-client-helper");
    fs::create_dir_all(&dir).expect("helper dir should be creatable");

    let source_path = dir.join("helper.rs");
    fs::write(&source_path, source).expect("helper source should be writable");

    let binary_path = dir.join("docker");
    let status = Command::new("rustc")
        .args([
            "--edition=2024",
            source_path.to_str().expect("source path should be utf-8"),
            "-o",
            binary_path.to_str().expect("binary path should be utf-8"),
        ])
        .status()
        .expect("rustc should be available");
    assert!(status.success(), "helper binary should compile");

    binary_path
}

fn args_file_script(args_file: &Path, exit_code: i32) -> String {
    r#"#!/bin/sh
set -eu
printf '%s\n' "$@" > "__ARGS_FILE__"
exit __EXIT_CODE__
    "#
    .replace("__ARGS_FILE__", &args_file.to_string_lossy())
    .replace("__EXIT_CODE__", &exit_code.to_string())
}

fn listing_script(inspect_json: &str) -> String {
    r#"#!/bin/sh
set -eu
case "$1" in
  ps)
    printf 'watchtower-id\nrunning-id\n'
    ;;
  inspect)
    cat <<'JSON'
__INSPECT_JSON__
JSON
    ;;
  *)
    exit 1
    ;;
esac
"#
    .replace("__INSPECT_JSON__", inspect_json)
}

fn get_container_binary(inspect_json: &str) -> PathBuf {
    let source = r#"
use std::env;
use std::process::exit;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("inspect") => print!("__INSPECT_JSON__"),
        Some("image") => exit(1),
        _ => exit(1),
    }
}
"#
    .replace("__INSPECT_JSON__", inspect_json);

    compile_helper_binary(&source)
}

fn stop_container_binary(state_file: &Path) -> PathBuf {
    let source = r#"
use std::env;
use std::fs;
use std::process::exit;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("kill") => {
            fs::write("__STATE_FILE__", "stopped").expect("state should be writable");
        }
        Some("rm") => {
            fs::write("__STATE_FILE__", "removed").expect("state should be writable");
        }
        Some("inspect") => {
            let state = fs::read_to_string("__STATE_FILE__").expect("state should be readable");
            match state.trim() {
                "removed" => print!("[]"),
                "stopped" => print!("__STOPPED_JSON__"),
                _ => print!("__RUNNING_JSON__"),
            }
        }
        _ => exit(1),
    }
}
"#
    .replace("__STATE_FILE__", &state_file.to_string_lossy())
    .replace("__STOPPED_JSON__", &stopped_container_json())
    .replace("__RUNNING_JSON__", &running_container_json());

    compile_helper_binary(&source)
}

fn inspect_json_array(values: Vec<serde_json::Value>) -> String {
    serde_json::Value::Array(values).to_string()
}

fn stopped_container_json() -> String {
    inspect_json_array(vec![container_inspect_entry(
        "container-id",
        "/demo",
        "",
        "demo:latest",
        false,
        false,
        &[],
        "default",
    )])
}

fn running_container_json() -> String {
    inspect_json_array(vec![container_inspect_entry(
        "container-id",
        "/demo",
        "",
        "demo:latest",
        true,
        false,
        &[],
        "default",
    )])
}

fn container_inspect_entry(
    id: &str,
    name: &str,
    image: &str,
    config_image: &str,
    running: bool,
    restarting: bool,
    labels: &[(&str, &str)],
    network_mode: &str,
) -> serde_json::Value {
    let labels = labels
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<BTreeMap<_, _>>();

    json!({
        "Id": id,
        "Name": name,
        "Created": "2026-06-20T11:00:00Z",
        "Image": image,
        "State": {
            "Running": running,
            "Restarting": restarting
        },
        "Config": {
            "Image": config_image,
            "Labels": labels,
            "Hostname": "demo-host"
        },
        "HostConfig": {
            "NetworkMode": network_mode,
            "AutoRemove": false
        },
        "NetworkSettings": {
            "Networks": {}
        },
        "Mounts": []
    })
}

#[test]
fn warn_on_head_pull_failed_matches_the_legacy_strategy_matrix() {
    let unknown = make_container(
        "container-id",
        "/unknown",
        "unknown.repo/prefix/imagename:latest",
        "sha256:current",
        &[],
        true,
        false,
        NetworkMode::Default,
        None,
    );
    let known = make_container(
        "container-id",
        "/known",
        "docker.io/prefix/imagename:latest",
        "sha256:current",
        &[],
        true,
        false,
        NetworkMode::Default,
        None,
    );

    assert!(warn_on_head_pull_failed(
        WarningStrategy::Always,
        unknown.image_name()
    ));
    assert!(warn_on_head_pull_failed(
        WarningStrategy::Auto,
        "ghcr.io/watchtower/image:main"
    ));
    assert!(!warn_on_head_pull_failed(
        WarningStrategy::Auto,
        unknown.image_name()
    ));
    assert!(warn_on_head_pull_failed_for_container(
        WarningStrategy::Auto,
        &known
    ));
    assert!(!warn_on_head_pull_failed_for_container(
        WarningStrategy::Never,
        &unknown
    ));
}

#[test]
fn pulling_a_pinned_digest_image_fails_closed_when_the_cli_rejects_it() {
    let args_file = unique_temp_path("pull-args");
    fs::write(&args_file, "").expect("args file should be writable");

    let script = args_file_script(&args_file, 1);
    let adapter = fake_docker_adapter(&script);
    let pinned = make_container(
        "container-id",
        "/demo",
        "sha256:fa5269854a5e615e51a72b17ad3fd1e01268f278a6684c8ed3c5f0cdce3f230b",
        "sha256:current",
        &[],
        true,
        false,
        NetworkMode::Default,
        None,
    );

    let err = adapter
        .is_container_stale(&pinned, &UpdateParams::default())
        .expect_err("a rejected pull should fail closed");

    assert!(matches!(err, DockerCliError::CommandFailed { .. }));
    let args = fs::read_to_string(&args_file).expect("pull args should be recorded");
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        vec![
            "pull",
            "sha256:fa5269854a5e615e51a72b17ad3fd1e01268f278a6684c8ed3c5f0cdce3f230b",
        ]
    );
}

#[test]
fn execute_command_uses_the_container_id_and_shell_wrapper() {
    let args_file = unique_temp_path("exec-args");
    fs::write(&args_file, "").expect("args file should be writable");

    let script = args_file_script(&args_file, 75);
    let adapter = fake_docker_adapter(&script);
    let container_id = ContainerID::from("ex-cont-id");

    let skip_update = adapter
        .execute_command(&container_id, "exec-cmd", 1)
        .expect("exec command should succeed");

    assert!(skip_update);
    let args = fs::read_to_string(&args_file).expect("exec args should be recorded");
    assert_eq!(
        args.lines().collect::<Vec<_>>(),
        vec!["exec", "ex-cont-id", "sh", "-c", "exec-cmd"]
    );
}

#[test]
fn stop_container_removes_the_running_container_and_tolerates_missing_after_removal() {
    let state_file = unique_temp_path("stop-state");
    fs::write(&state_file, "running").expect("state file should be writable");

    let binary = stop_container_binary(&state_file);
    let adapter = DockerCliAdapter::with_binary(binary.to_string_lossy().into_owned());
    let container = make_container(
        "container-id",
        "/demo",
        "demo:latest",
        "sha256:current",
        &[],
        true,
        false,
        NetworkMode::Default,
        None,
    );

    adapter
        .stop_container(&container, Duration::from_millis(100))
        .expect("stop should succeed");

    assert_eq!(
        fs::read_to_string(&state_file).expect("state should be readable"),
        "removed"
    );
}

#[test]
fn remove_image_by_id_calls_image_rm_and_fails_closed_on_cli_errors() {
    let args_file = unique_temp_path("remove-image-args");
    fs::write(&args_file, "").expect("args file should be writable");

    let success_script = args_file_script(&args_file, 0);
    let adapter = fake_docker_adapter(&success_script);
    let image_id = ImageID::from("sha256:deadbeef");

    adapter
        .remove_image_by_id(&image_id)
        .expect("image removal should succeed");
    let args = fs::read_to_string(&args_file).expect("remove args should be recorded");
    assert_eq!(args.lines().collect::<Vec<_>>(), vec!["image", "rm", "sha256:deadbeef"]);

    let failure_args_file = unique_temp_path("remove-image-failure-args");
    fs::write(&failure_args_file, "").expect("args file should be writable");
    let failure_script = args_file_script(&failure_args_file, 1);
    let failing_adapter = fake_docker_adapter(&failure_script);

    let err = failing_adapter
        .remove_image_by_id(&ImageID::from("sha256:missing"))
        .expect_err("missing images should fail closed");

    assert!(matches!(err, DockerCliError::CommandFailed { .. }));
}

#[test]
fn list_containers_and_filters_match_the_legacy_selection_cases() {
    let inspect_json = inspect_json_array(vec![
        container_inspect_entry(
            "watchtower-id",
            "/watchtower",
            "",
            "marrrrrrrrry/watchtower:latest",
            true,
            false,
            &[("com.centurylinklabs.watchtower", "true")],
            "default",
        ),
        container_inspect_entry(
            "running-id",
            "/running",
            "",
            "docker.io/prefix/imagename:latest",
            true,
            false,
            &[],
            "default",
        ),
    ]);
    let script = listing_script(&inspect_json);
    let adapter = fake_docker_adapter(&script);

    let containers = adapter.list_containers().expect("containers should list");
    assert_eq!(containers.len(), 2);

    let watchtower = vec![
        FilterContainer {
            name: "/watchtower".to_string(),
            watchtower: true,
            image_name: "marrrrrrrrry/watchtower:latest".to_string(),
        },
        FilterContainer {
            name: "/running".to_string(),
            watchtower: false,
            image_name: "docker.io/prefix/imagename:latest".to_string(),
        },
    ];
    let all = names_matching(&watchtower, Box::new(filters::no_filter::<FilterContainer>));
    assert_eq!(all, vec!["/watchtower", "/running"]);

    let empty_names = vec!["lollercoaster".to_string()];
    let empty_filter = filters::filter_by_names(
        &empty_names,
        Box::new(filters::no_filter::<FilterContainer>),
    );
    assert!(names_matching(&watchtower, empty_filter).is_empty());

    let watchtower_filter = Box::new(filters::watchtower_containers_filter::<FilterContainer>);
    assert_eq!(
        names_matching(&watchtower, watchtower_filter),
        vec!["/watchtower"]
    );

    assert_eq!(containers[0].image_name(), "marrrrrrrrry/watchtower:latest");
}

#[test]
fn container_list_statuses_match_the_legacy_flag_matrix() {
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
fn normalize_network_config_removes_container_id_aliases_only() {
    let mut endpoints = HashMap::new();
    endpoints.insert(
        "bridge".to_string(),
        NetworkEndpoint {
            aliases: vec!["abc123".to_string(), "db".to_string(), "redis".to_string()],
        },
    );
    endpoints.insert(
        "other".to_string(),
        NetworkEndpoint {
            aliases: vec!["abc123".to_string(), "cache".to_string()],
        },
    );

    let normalized = normalize_network_config(NetworkingConfig { endpoints }, "abc123");

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
    endpoints.insert(
        "bridge".to_string(),
        NetworkEndpoint {
            aliases: vec!["db".to_string()],
        },
    );
    endpoints.insert(
        "other".to_string(),
        NetworkEndpoint {
            aliases: vec!["cache".to_string()],
        },
    );

    let simple = simple_network_config(&NetworkingConfig { endpoints });

    assert_eq!(simple.endpoints.len(), 1);
    let endpoint = simple.endpoints.values().next().unwrap();
    assert!(endpoint.aliases == vec!["db".to_string()] || endpoint.aliases == vec!["cache".to_string()]);
}

#[test]
fn get_network_config_strips_the_container_id_alias() {
    let container = container_with_aliases(
        "docker.io/prefix/imagename:latest",
        &["One", "Two", "1234567890ab", "Four"],
    );
    let network_config = container.get_network_config();

    let endpoint = network_config.endpoints.get("test").expect("test endpoint");
    assert_eq!(
        endpoint.aliases,
        vec!["One".to_string(), "Two".to_string(), "Four".to_string()]
    );
}

#[test]
fn get_container_preserves_container_network_mode_when_it_is_a_container_link() {
    let inspect_json = inspect_json_array(vec![container_inspect_entry(
        "container-id",
        "/demo",
        "sha256:current",
        "docker.io/prefix/imagename:latest",
        true,
        false,
        &[],
        "container:network-supplier-id",
    )]);
    let binary = get_container_binary(&inspect_json);
    let adapter = DockerCliAdapter::with_binary(binary.to_string_lossy().into_owned());

    let container = adapter
        .get_container(&ContainerID::from("container-id"))
        .expect("container should be returned");

    let network_mode = container
        .container_info()
        .and_then(|info| info.host_config.as_ref())
        .map(|host_config| &host_config.network_mode)
        .expect("network mode should exist");
    assert_eq!(
        network_mode.connected_container(),
        Some("network-supplier-id")
    );
}
