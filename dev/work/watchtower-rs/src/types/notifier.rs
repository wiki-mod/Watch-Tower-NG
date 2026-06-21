#![forbid(unsafe_code)]

//! Notifier trait shared by all notification services.
//!
//! Translated from `old-source/pkg/types/notifier.go`.

use super::report::Report;

/// Notification surface shared by the legacy notifier implementations.
pub trait Notifier {
    fn start_notification(&self);
    fn send_notification(&self, report: &Report);
    fn add_log_hook(&self);
    fn get_names(&self) -> Vec<String>;
    fn get_urls(&self) -> Vec<String>;
    fn close(&self);
}

#[cfg(test)]
mod tests {
    use super::super::container::ContainerID;
    use super::super::container::ImageID;
    use super::super::report::ContainerReport;
    use super::*;
    use std::cell::{Cell, RefCell};

    struct MockNotifier {
        started: Cell<bool>,
        hook_added: Cell<bool>,
        closed: Cell<bool>,
        names: Vec<String>,
        urls: Vec<String>,
        reports: RefCell<Vec<Report>>,
    }

    impl Notifier for MockNotifier {
        fn start_notification(&self) {
            self.started.set(true);
        }

        fn send_notification(&self, report: &Report) {
            self.reports.borrow_mut().push(report.clone());
        }

        fn add_log_hook(&self) {
            self.hook_added.set(true);
        }

        fn get_names(&self) -> Vec<String> {
            self.names.clone()
        }

        fn get_urls(&self) -> Vec<String> {
            self.urls.clone()
        }

        fn close(&self) {
            self.closed.set(true);
        }
    }

    #[test]
    fn notifier_trait_exposes_legacy_surface() {
        let notifier = MockNotifier {
            started: Cell::new(false),
            hook_added: Cell::new(false),
            closed: Cell::new(false),
            names: vec!["logger".to_string()],
            urls: vec!["stdout://".to_string()],
            reports: RefCell::new(Vec::new()),
        };
        let report = Report {
            scanned: vec![ContainerReport {
                id: ContainerID::from("a"),
                name: "name-a".to_string(),
                current_image_id: ImageID::from("old-a"),
                latest_image_id: ImageID::from("new-a"),
                image_name: "image-a".to_string(),
                error: None,
                state: "Scanned".to_string(),
            }],
            ..Report::default()
        };

        notifier.start_notification();
        notifier.send_notification(&report);
        notifier.add_log_hook();
        notifier.close();

        assert!(notifier.started.get());
        assert!(notifier.hook_added.get());
        assert!(notifier.closed.get());
        assert_eq!(notifier.get_names(), vec!["logger".to_string()]);
        assert_eq!(notifier.get_urls(), vec!["stdout://".to_string()]);
        assert_eq!(notifier.reports.borrow().as_slice(), &[report]);
    }
}
