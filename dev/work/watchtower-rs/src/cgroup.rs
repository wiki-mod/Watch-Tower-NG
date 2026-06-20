#![forbid(unsafe_code)]

use std::fs;
use std::io;
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

pub fn get_running_container_id() -> io::Result<Option<ContainerID>> {
    let file = fs::read_to_string(format!("/proc/{}/cgroup", process::id()))?;
    Ok(get_running_container_id_from_string(&file))
}

pub fn get_running_container_id_from_string(contents: &str) -> Option<ContainerID> {
    docker_container_pattern()
        .captures(contents)
        .and_then(|matches| matches.get(1))
        .map(|matched| ContainerID::new(matched.as_str()))
}
