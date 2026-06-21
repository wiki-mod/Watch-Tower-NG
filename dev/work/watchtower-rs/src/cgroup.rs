#![forbid(unsafe_code)]

use std::fs;
use std::process;
use std::sync::OnceLock;

use regex::Regex;

use crate::types::ContainerID;

static DOCKER_CONTAINER_PATTERN: OnceLock<Regex> = OnceLock::new();

fn docker_container_pattern() -> &'static Regex {
    DOCKER_CONTAINER_PATTERN.get_or_init(|| {
        Regex::new(r"[0-9]+:.*:/docker/([a-f|0-9]{64})")
            .expect("docker container regex should compile")
    })
}

/// Get the running container ID from the current process cgroup information.
/// Returns None if the process is not running in a container.
/// If cgroup cannot be read, treats it as an empty string for pattern matching.
pub fn get_running_container_id() -> Option<ContainerID> {
    let file = match fs::read_to_string(format!("/proc/{}/cgroup", process::id())) {
        Ok(content) => content,
        Err(_) => String::new(),
    };
    get_running_container_id_from_string(&file)
}

/// Extract container ID from cgroup string.
/// Returns None if the container ID pattern is not found in the string.
pub fn get_running_container_id_from_string(contents: &str) -> Option<ContainerID> {
    docker_container_pattern()
        .captures(contents)
        .and_then(|matches| matches.get(1))
        .map(|matched| ContainerID::new(matched.as_str()))
}
