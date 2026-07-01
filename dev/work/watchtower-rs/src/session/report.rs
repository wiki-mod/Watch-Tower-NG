#![forbid(unsafe_code)]

use super::container_status::State;
use super::progress::Progress;
use crate::types::{ContainerReport, Report};

/// Build a `Report` from the current session progress.
pub(super) fn new_report(progress: &Progress) -> Report {
    let mut report = Report::default();

    for status in progress.0.values().cloned() {
        let state = status.state;

        if state == State::Skipped {
            report.skipped.push(status.into_report());
            continue;
        }

        let mut report_entry = status.into_report();
        report.scanned.push(report_entry.clone());

        if report_entry.current_image_id == report_entry.latest_image_id {
            report_entry.state = State::Fresh.as_str().to_string();
            report.fresh.push(report_entry);
            continue;
        }

        match state {
            State::Updated => report.updated.push(report_entry),
            State::Failed => report.failed.push(report_entry),
            State::Unknown | State::Scanned | State::Fresh | State::Stale => {
                report_entry.state = State::Stale.as_str().to_string();
                report.stale.push(report_entry);
            }
            State::Skipped => unreachable!("skipped entries are handled earlier"),
        }
    }

    sort_reports(&mut report.scanned);
    sort_reports(&mut report.updated);
    sort_reports(&mut report.failed);
    sort_reports(&mut report.skipped);
    sort_reports(&mut report.stale);
    sort_reports(&mut report.fresh);

    report
}

/// Sort a slice of `ContainerReport` by container ID.
pub(super) fn sort_reports(reports: &mut [ContainerReport]) {
    reports.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
}

#[cfg(test)]
mod tests {
    use super::super::container_status::ContainerLike;
    use super::super::progress::Progress;
    use crate::types::{ContainerID, ImageID};

    #[derive(Clone)]
    struct MockContainer {
        id: ContainerID,
        name: String,
        image_name: String,
        current_image_id: ImageID,
    }

    impl ContainerLike for MockContainer {
        fn id(&self) -> &ContainerID {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn image_name(&self) -> &str {
            &self.image_name
        }

        fn current_image_id(&self) -> &ImageID {
            &self.current_image_id
        }

        fn safe_image_id(&self) -> ImageID {
            self.current_image_id.clone()
        }
    }

    fn container(id: &str, name: &str, image_name: &str, current_image_id: &str) -> MockContainer {
        MockContainer {
            id: ContainerID::from(id),
            name: name.to_string(),
            image_name: image_name.to_string(),
            current_image_id: ImageID::from(current_image_id),
        }
    }

    #[test]
    fn report_conversion_sorts_and_classifies_buckets() {
        let fresh = container("d", "fresh", "image:fresh", "same");
        let updated = container("c", "updated", "image:updated", "old");
        let failed = container("b", "failed", "image:failed", "old");
        let skipped = container("e", "skipped", "image:skipped", "old");
        let stale = container("a", "stale", "image:stale", "old");

        let mut progress = Progress::default();
        progress.add_scanned(&stale, "new-stale");
        progress.add_scanned(&failed, "new-failed");
        progress.add_scanned(&updated, "new-updated");
        progress.add_scanned(&fresh, "same");
        progress.add_scanned(&skipped, "new-skipped");

        progress.mark_for_update(updated.id());
        progress.mark_for_update(failed.id());
        progress.update_failed([(failed.id().clone(), "network error")]);
        progress.add_skipped(&skipped, "container disappeared");

        let report = progress.report();

        assert_eq!(
            report
                .scanned
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c", "d"]
        );
        assert_eq!(
            report
                .updated
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c"]
        );
        assert_eq!(
            report
                .failed
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
        assert_eq!(
            report
                .skipped
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["e"]
        );
        assert_eq!(
            report
                .fresh
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["d"]
        );
        assert_eq!(
            report
                .stale
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(report.stale[0].state, "Stale");
    }
}
