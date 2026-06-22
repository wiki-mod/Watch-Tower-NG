#![forbid(unsafe_code)]

//! Running container ID detection via cgroup.
//!
//! Translated from `old-source/pkg/container/cgroup_id.go`.

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
///
/// Returns `Ok(Some(id))` if found, `Ok(None)` if the process is not in a
/// Docker container, or `Err` if the cgroup file cannot be read.
///
/// Mirrors Go's `GetRunningContainerID` which propagates file read errors.
pub fn get_running_container_id() -> Result<Option<ContainerID>, std::io::Error> {
    let file = fs::read_to_string(format!("/proc/{}/cgroup", process::id()))?;
    Ok(get_running_container_id_from_string(&file))
}

/// Extract container ID from cgroup string.
/// Returns `Some(id)` if the Docker container ID pattern is found, `None` otherwise.
pub fn get_running_container_id_from_string(contents: &str) -> Option<ContainerID> {
    docker_container_pattern()
        .captures(contents)
        .and_then(|matches| matches.get(1))
        .map(|matched| ContainerID::new(matched.as_str()))
}
