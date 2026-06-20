#![forbid(unsafe_code)]

//! Helpers for resolving the current Docker container ID from cgroup data.
//!
//! The parser mirrors the legacy Go behavior closely: it only accepts
//! Docker-style cgroup lines and returns no ID on any mismatch.

use std::fs;
use std::io;
use std::process;

use crate::types::ContainerID;

const DOCKER_CGROUP_MARKER: &str = ":/docker/";
const DOCKER_ID_LEN: usize = 64;

/// Read `/proc/<pid>/cgroup` for the current process and extract a container ID.
pub fn get_running_container_id() -> io::Result<Option<ContainerID>> {
    let pid = process::id();
    let path = format!("/proc/{pid}/cgroup");
    let contents = fs::read_to_string(path)?;
    Ok(get_running_container_id_from_string(&contents))
}

/// Extract a Docker container ID from raw cgroup contents.
///
/// The function is intentionally fail-closed: if the input does not contain a
/// Docker cgroup entry with a 64-character lowercase hex ID, it returns `None`.
pub fn get_running_container_id_from_string(contents: &str) -> Option<ContainerID> {
    contents
        .lines()
        .find_map(parse_container_id_from_cgroup_line)
        .map(ContainerID::new)
}

fn parse_container_id_from_cgroup_line(line: &str) -> Option<&str> {
    let (cgroup_index, remainder) = line.split_once(':')?;
    if cgroup_index.is_empty() || !cgroup_index.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let docker_start = remainder.find(DOCKER_CGROUP_MARKER)? + DOCKER_CGROUP_MARKER.len();
    let candidate = remainder.get(docker_start..docker_start + DOCKER_ID_LEN)?;

    if candidate.chars().all(is_lower_hex) {
        Some(candidate)
    } else {
        None
    }
}

fn is_lower_hex(ch: char) -> bool {
    ch.is_ascii_digit() || matches!(ch, 'a'..='f')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_docker_container_id_from_matching_cgroup_line() {
        let contents = r#"
15:name=systemd:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
14:misc:/
13:rdma:/docker/991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377
"#;

        let container_id = get_running_container_id_from_string(contents)
            .expect("expected a container id to be extracted");

        assert_eq!(
            container_id.as_str(),
            "991b6b42691449d3ce90192ff9f006863dcdafc6195e227aeefa298235004377"
        );
    }

    #[test]
    fn returns_none_when_no_matching_container_id_is_present() {
        assert_eq!(get_running_container_id_from_string("14:misc:/"), None);
    }
}
