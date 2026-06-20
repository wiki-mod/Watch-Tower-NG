#![allow(dead_code)]

use std::collections::HashSet;

use crate::types::{ContainerID, ContainerReport};

/// State is the outcome of a container in a session report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Scanned,
    Updated,
    Failed,
    Skipped,
    Stale,
    Fresh,
}

impl State {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scanned => "scanned",
            Self::Updated => "updated",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Stale => "stale",
            Self::Fresh => "fresh",
        }
    }
}

/// Parses a string of state characters and returns a slice of the corresponding report states.
pub fn states_from_string(input: &str) -> Vec<State> {
    let mut states = Vec::with_capacity(input.len());
    for c in input.chars() {
        match c {
            'c' => states.push(State::Scanned),
            'u' => states.push(State::Updated),
            'e' => states.push(State::Failed),
            'k' => states.push(State::Skipped),
            't' => states.push(State::Stale),
            'f' => states.push(State::Fresh),
            _ => continue,
        }
    }
    states
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub scanned: Vec<ContainerReport>,
    pub updated: Vec<ContainerReport>,
    pub failed: Vec<ContainerReport>,
    pub skipped: Vec<ContainerReport>,
    pub stale: Vec<ContainerReport>,
    pub fresh: Vec<ContainerReport>,
}

impl Report {
    pub fn scanned(&self) -> &[ContainerReport] {
        &self.scanned
    }

    pub fn updated(&self) -> &[ContainerReport] {
        &self.updated
    }

    pub fn failed(&self) -> &[ContainerReport] {
        &self.failed
    }

    pub fn skipped(&self) -> &[ContainerReport] {
        &self.skipped
    }

    pub fn stale(&self) -> &[ContainerReport] {
        &self.stale
    }

    pub fn fresh(&self) -> &[ContainerReport] {
        &self.fresh
    }

    pub fn all(&self) -> Vec<ContainerReport> {
        let mut all = Vec::with_capacity(
            self.scanned.len()
                + self.updated.len()
                + self.failed.len()
                + self.skipped.len()
                + self.stale.len()
                + self.fresh.len(),
        );

        let mut present_ids: HashSet<ContainerID> = HashSet::new();
        let mut append_unique = |reports: &[ContainerReport]| {
            for cr in reports {
                if present_ids.contains(cr.id()) {
                    continue;
                }
                all.push(cr.clone());
                present_ids.insert(cr.id.clone());
            }
        };

        append_unique(&self.updated);
        append_unique(&self.failed);
        append_unique(&self.skipped);
        append_unique(&self.stale);
        append_unique(&self.fresh);
        append_unique(&self.scanned);

        all.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContainerID, ImageID};

    fn container_report(id: &str) -> ContainerReport {
        ContainerReport {
            id: ContainerID::from(id),
            name: format!("name-{id}"),
            current_image_id: ImageID::from("current"),
            latest_image_id: ImageID::from("latest"),
            image_name: format!("image-{id}"),
            error: None,
            state: "state".to_string(),
        }
    }

    #[test]
    fn parses_state_characters_like_legacy_go() {
        assert_eq!(
            states_from_string("cuektfz"),
            vec![
                State::Scanned,
                State::Updated,
                State::Failed,
                State::Skipped,
                State::Stale,
                State::Fresh,
            ]
        );
    }

    #[test]
    fn all_deduplicates_by_priority_and_sorts_by_id() {
        let mut report = Report::default();
        report.scanned = vec![container_report("b"), container_report("a")];
        report.updated = vec![container_report("c"), container_report("a")];
        report.failed = vec![container_report("d")];

        let all = report.all();
        assert_eq!(
            all.into_iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![
                ContainerID::from("a"),
                ContainerID::from("b"),
                ContainerID::from("c"),
                ContainerID::from("d"),
            ]
        );
    }
}
