#![forbid(unsafe_code)]

//! Report and ContainerReport types for session results.
//!
//! Translated from `old-source/pkg/types/report.go`.

use serde::{Deserialize, Serialize};

use super::container::{ContainerID, ImageID};

/// A single container entry in an update report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerReport {
    pub id: ContainerID,
    pub name: String,
    pub current_image_id: ImageID,
    pub latest_image_id: ImageID,
    pub image_name: String,
    pub error: Option<String>,
    pub state: String,
}

impl ContainerReport {
    /// Return the reported container ID.
    pub fn id(&self) -> &ContainerID {
        &self.id
    }

    /// Return the reported container name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Return the current image ID.
    pub fn current_image_id(&self) -> &ImageID {
        &self.current_image_id
    }

    /// Return the latest image ID.
    pub fn latest_image_id(&self) -> &ImageID {
        &self.latest_image_id
    }

    /// Return the image name associated with the report.
    pub fn image_name(&self) -> &str {
        self.image_name.as_str()
    }

    /// Return the recorded error text, or an empty string when the report has no error.
    pub fn error(&self) -> &str {
        self.error.as_deref().unwrap_or("")
    }

    /// Return the recorded state string.
    pub fn state(&self) -> &str {
        self.state.as_str()
    }

    /// True when the report recorded an error.
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Aggregated session report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub scanned: Vec<ContainerReport>,
    pub updated: Vec<ContainerReport>,
    pub failed: Vec<ContainerReport>,
    pub skipped: Vec<ContainerReport>,
    pub stale: Vec<ContainerReport>,
    pub fresh: Vec<ContainerReport>,
}

impl Report {
    /// Return the scanned entries in their recorded order.
    pub fn scanned(&self) -> &[ContainerReport] {
        &self.scanned
    }

    /// Return the updated entries in their recorded order.
    pub fn updated(&self) -> &[ContainerReport] {
        &self.updated
    }

    /// Return the failed entries in their recorded order.
    pub fn failed(&self) -> &[ContainerReport] {
        &self.failed
    }

    /// Return the skipped entries in their recorded order.
    pub fn skipped(&self) -> &[ContainerReport] {
        &self.skipped
    }

    /// Return the stale entries in their recorded order.
    pub fn stale(&self) -> &[ContainerReport] {
        &self.stale
    }

    /// Return the fresh entries in their recorded order.
    pub fn fresh(&self) -> &[ContainerReport] {
        &self.fresh
    }

    /// Return every recorded container entry once, in deterministic ID order.
    ///
    /// The legacy Go report deduplicated by container ID with the priority order
    /// `updated`, `failed`, `skipped`, `stale`, `fresh`, `scanned`.
    pub fn all(&self) -> Vec<ContainerReport> {
        self.all_refs().into_iter().cloned().collect()
    }

    /// Return borrowed views of every recorded container entry once, in
    /// deterministic ID order.
    pub fn all_refs(&self) -> Vec<&ContainerReport> {
        let mut all = Vec::with_capacity(
            self.scanned.len()
                + self.updated.len()
                + self.failed.len()
                + self.skipped.len()
                + self.stale.len()
                + self.fresh.len(),
        );
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

        for bucket in [
            &self.updated,
            &self.failed,
            &self.skipped,
            &self.stale,
            &self.fresh,
            &self.scanned,
        ] {
            for report in bucket {
                if seen.insert(report.id.as_str()) {
                    all.push(report);
                }
            }
        }

        all.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        all
    }

    /// Return true when the report has no recorded entries.
    pub fn is_empty(&self) -> bool {
        self.all().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_all_deduplicates_by_priority_and_sorts_by_id() {
        let make = |id: &str, state: &str| ContainerReport {
            id: ContainerID::from(id),
            name: format!("name-{id}"),
            current_image_id: ImageID::from(format!("old-{id}")),
            latest_image_id: ImageID::from(format!("new-{id}")),
            image_name: format!("image-{id}"),
            error: None,
            state: state.to_string(),
        };

        let report = Report {
            scanned: vec![make("c", "Scanned"), make("a", "Scanned")],
            updated: vec![make("b", "Updated"), make("a", "Updated")],
            failed: vec![make("d", "Failed")],
            skipped: vec![make("e", "Skipped")],
            stale: vec![make("f", "Stale")],
            fresh: vec![make("g", "Fresh")],
        };

        let ids = report
            .all()
            .into_iter()
            .map(|entry| entry.id.to_string())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["a", "b", "c", "d", "e", "f", "g"]);
        assert!(!report.is_empty());
    }

    #[test]
    fn report_all_returns_owned_reports() {
        let report = Report {
            updated: vec![ContainerReport {
                id: ContainerID::from("a"),
                name: "name-a".to_string(),
                current_image_id: ImageID::from("old-a"),
                latest_image_id: ImageID::from("new-a"),
                image_name: "image-a".to_string(),
                error: None,
                state: "Updated".to_string(),
            }],
            ..Report::default()
        };

        let mut all = report.all();
        all[0].state = "Mutated".to_string();

        assert_eq!(report.updated()[0].state, "Updated");
        assert_eq!(report.all_refs()[0].state, "Updated");
    }

    #[test]
    fn container_report_accessors_match_legacy_shape() {
        let report = ContainerReport {
            id: ContainerID::from("container-id"),
            name: "name".to_string(),
            current_image_id: ImageID::from("old-image"),
            latest_image_id: ImageID::from("new-image"),
            image_name: "example/image:latest".to_string(),
            error: Some("boom".to_string()),
            state: "Failed".to_string(),
        };

        assert_eq!(report.id().as_str(), "container-id");
        assert_eq!(report.name(), "name");
        assert_eq!(report.current_image_id().as_str(), "old-image");
        assert_eq!(report.latest_image_id().as_str(), "new-image");
        assert_eq!(report.image_name(), "example/image:latest");
        assert_eq!(report.error(), "boom");
        assert_eq!(report.state(), "Failed");
        assert!(report.has_error());

        let no_error = ContainerReport {
            error: None,
            ..report
        };

        assert_eq!(no_error.error(), "");
        assert!(!no_error.has_error());
    }

    #[test]
    fn report_bucket_accessors_preserve_recorded_order() {
        let make = |id: &str, state: &str| ContainerReport {
            id: ContainerID::from(id),
            name: format!("name-{id}"),
            current_image_id: ImageID::from(format!("old-{id}")),
            latest_image_id: ImageID::from(format!("new-{id}")),
            image_name: format!("image-{id}"),
            error: None,
            state: state.to_string(),
        };

        let report = Report {
            scanned: vec![make("b", "Scanned"), make("a", "Scanned")],
            updated: vec![make("c", "Updated")],
            failed: vec![make("d", "Failed")],
            skipped: vec![make("e", "Skipped")],
            stale: vec![make("f", "Stale")],
            fresh: vec![make("g", "Fresh")],
        };

        assert_eq!(
            report
                .scanned()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert_eq!(
            report
                .updated()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
        assert_eq!(
            report
                .failed()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["d"]
        );
        assert_eq!(
            report
                .skipped()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["e"]
        );
        assert_eq!(
            report
                .stale()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["f"]
        );
        assert_eq!(
            report
                .fresh()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["g"]
        );
    }
}
