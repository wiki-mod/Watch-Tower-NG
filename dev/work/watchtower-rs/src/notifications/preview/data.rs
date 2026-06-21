//! Legacy preview-data generator translated from `old-source/pkg/notifications/preview/data/data.go`.
//!
//! The root preview module wires this file together with the sibling split
//! helpers, but the deterministic sample-data behavior lives here.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as _;

use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};
use time::{Duration, OffsetDateTime};

use crate::types::{ContainerID, ContainerReport, ImageID};

use super::logs::{LogEntry, LogLevel};
use super::preview_strings::{
    CONTAINER_NAMES, ERROR_MESSAGES, LOG_ERRORS, LOG_MESSAGES, ORGANIZATION_NAMES,
    SKIPPED_MESSAGES,
};
use super::report::Report;
use super::report::State;
use super::status::containerStatus;

/// Static fields exposed to notification templates during preview rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StaticData {
    pub title: String,
    pub host: String,
}

impl StaticData {
    fn legacy() -> Self {
        Self {
            title: "Title".to_string(),
            host: "Host".to_string(),
        }
    }
}

impl Default for StaticData {
    fn default() -> Self {
        Self::legacy()
    }
}

/// Deterministic preview data generator used by the notification preview slice.
#[derive(Clone, Debug)]
pub(crate) struct PreviewData {
    rng: StdRng,
    last_time: OffsetDateTime,
    report: Option<Report>,
    container_count: usize,
    pub entries: Vec<LogEntry>,
    pub static_data: StaticData,
}

impl PreviewData {
    /// Initialize a new preview-data bundle with the legacy seed and sample metadata.
    pub(crate) fn new() -> Self {
        Self {
            rng: StdRng::seed_from_u64(1),
            last_time: OffsetDateTime::now_utc() - Duration::minutes(30),
            report: None,
            container_count: 0,
            entries: Vec::new(),
            static_data: StaticData::default(),
        }
    }

    /// Add a container status entry to the preview report with the given state.
    pub(crate) fn add_from_state(&mut self, state: State) {
        let cid = ContainerID::from(self.generate_id());
        let old = ImageID::from(self.generate_id());
        let new = ImageID::from(self.generate_id());
        let name = self.generate_name();
        let image = self.generate_image_name(&name);
        let error = match state {
            State::Failed => Some(self.random_entry(ERROR_MESSAGES).to_string()),
            State::Skipped => Some(self.random_entry(SKIPPED_MESSAGES).to_string()),
            _ => None,
        };

        self.add_container(containerStatus::new(cid, old, new, name, image, error, state));
    }

    fn add_container(&mut self, container: containerStatus) {
        let report = self.report.get_or_insert_with(Report::default);
        let state = container.state().to_owned();
        let report_entry = ContainerReport {
            id: container.id().clone(),
            name: container.name().to_string(),
            current_image_id: container.current_image_id().clone(),
            latest_image_id: container.latest_image_id().clone(),
            image_name: container.image_name().to_string(),
            error: {
                let error = container.error();
                if error.is_empty() {
                    None
                } else {
                    Some(error.to_string())
                }
            },
            state,
        };

        match container.state() {
            "scanned" => report.scanned.push(report_entry),
            "updated" => report.updated.push(report_entry),
            "failed" => report.failed.push(report_entry),
            "skipped" => report.skipped.push(report_entry),
            "stale" => report.stale.push(report_entry),
            "fresh" => report.fresh.push(report_entry),
            _ => return,
        }
        self.container_count += 1;
    }

    /// Add a deterministic preview log entry of the given level.
    pub(crate) fn add_log_entry(&mut self, level: LogLevel) {
        let message = match level {
            LogLevel::Fatal | LogLevel::Error | LogLevel::Warn => {
                self.random_entry(LOG_ERRORS).to_string()
            }
            _ => self.random_entry(LOG_MESSAGES).to_string(),
        };
        let time = self.generate_time();

        self.entries.push(LogEntry {
            message,
            data: HashMap::new(),
            time,
            level,
        });
    }

    /// Return the preview report when at least one state was recorded.
    pub(crate) fn report(&self) -> Option<Report> {
        self.report.clone()
    }

    fn generate_id(&mut self) -> String {
        let mut buf = [0u8; 32];
        self.rng.fill_bytes(&mut buf);
        hex_encode(&buf)
    }

    fn generate_time(&mut self) -> OffsetDateTime {
        self.last_time += Duration::seconds(self.rng.gen_range(0..30) as i64);
        self.last_time
    }

    fn random_entry<'a>(&mut self, entries: &'a [&'a str]) -> &'a str {
        let index = self.rng.gen_range(0..entries.len());
        entries[index]
    }

    fn generate_name(&self) -> String {
        let index = self.container_count;
        if index <= CONTAINER_NAMES.len() {
            // Keep the legacy off-by-one guard exactly as written in Go for parity.
            return format!("/{}", CONTAINER_NAMES[index]);
        }

        let suffix = index / CONTAINER_NAMES.len();
        let slot = index % CONTAINER_NAMES.len();
        format!("/{}{}", CONTAINER_NAMES[slot], suffix)
    }

    fn generate_image_name(&self, name: &str) -> String {
        let index = self.container_count % ORGANIZATION_NAMES.len();
        format!("{}{}:latest", ORGANIZATION_NAMES[index], name)
    }
}

impl Default for PreviewData {
    fn default() -> Self {
        Self::new()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_data_starts_with_legacy_defaults() {
        let data = PreviewData::new();

        assert_eq!(data.container_count, 0);
        assert!(data.report.is_none());
        assert_eq!(data.static_data.title, "Title");
        assert_eq!(data.static_data.host, "Host");
        assert!(data.entries.is_empty());
    }

    #[test]
    fn preview_data_adds_reports_and_logs_deterministically() {
        let mut data = PreviewData::new();

        data.add_from_state(State::Scanned);
        data.add_from_state(State::Failed);
        data.add_log_entry(LogLevel::Error);

        let report = data.report().expect("preview report should exist");
        assert_eq!(report.scanned.len(), 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.updated.len(), 0);
        assert_eq!(report.all().len(), 2);
        assert_eq!(report.scanned()[0].name(), "/cyberscribe");
        assert_eq!(report.failed()[0].name(), "/datamatrix");

        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].level, LogLevel::Error);
        assert!(!data.entries[0].message.is_empty());
        assert!(data.entries[0].data.is_empty());
    }

    #[test]
    fn preview_container_status_accessors_match_legacy_shape() {
        let status = containerStatus::new(
            ContainerID::from("cid"),
            ImageID::from("old"),
            ImageID::from("new"),
            "/name".to_string(),
            "repo/name:latest".to_string(),
            Some("boom".to_string()),
            State::Updated,
        );

        assert_eq!(status.id().as_str(), "cid");
        assert_eq!(status.name(), "/name");
        assert_eq!(status.current_image_id().as_str(), "old");
        assert_eq!(status.latest_image_id().as_str(), "new");
        assert_eq!(status.image_name(), "repo/name:latest");
        assert_eq!(status.error(), "boom");
        assert_eq!(status.state(), "updated");
    }
}
