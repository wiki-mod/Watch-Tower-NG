use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::types::{ContainerReport, Report};

/// Static notification fields that are resolved once per notifier instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaticData {
    pub title: String,
    pub host: String,
}

/// Pure input bundle that replaces the legacy Cobra flag lookups.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateDataInput {
    pub configured_hostname: Option<String>,
    pub fallback_hostname: Option<String>,
    pub skip_title: bool,
    pub title_tag: Option<String>,
    pub legacy_email_subject_tag: Option<String>,
}

impl TemplateDataInput {
    fn hostname(&self) -> String {
        self.configured_hostname
            .as_deref()
            .filter(|hostname| !hostname.is_empty())
            .or_else(|| {
                self.fallback_hostname
                    .as_deref()
                    .filter(|hostname| !hostname.is_empty())
            })
            .unwrap_or_default()
            .to_string()
    }

    fn title_tag(&self) -> String {
        self.title_tag
            .as_deref()
            .filter(|tag| !tag.is_empty())
            .or_else(|| {
                self.legacy_email_subject_tag
                    .as_deref()
                    .filter(|tag| !tag.is_empty())
            })
            .unwrap_or_default()
            .to_string()
    }
}

/// One log entry captured for notification templates.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationEntry {
    pub level: String,
    pub message: String,
    pub data: Option<Value>,
    pub time: String,
}

impl NotificationEntry {
    /// Create a new notification entry.
    pub fn new(
        level: impl Into<String>,
        message: impl Into<String>,
        data: Option<Value>,
        time: impl Into<String>,
    ) -> Self {
        Self {
            level: level.into(),
            message: message.into(),
            data,
            time: time.into(),
        }
    }

    fn to_json_value(&self) -> Value {
        json!({
            "level": self.level,
            "message": self.message,
            "data": self.data,
            "time": self.time,
        })
    }
}

/// Notification template payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Data {
    pub static_data: StaticData,
    pub entries: Vec<NotificationEntry>,
    pub report: Option<Report>,
}

impl Data {
    /// Build a payload from its static template data, entries and optional report.
    pub fn new(
        static_data: StaticData,
        entries: Vec<NotificationEntry>,
        report: Option<Report>,
    ) -> Self {
        Self {
            static_data,
            entries,
            report,
        }
    }

    /// Serialize the payload into the legacy notification JSON shape.
    pub fn to_json_value(&self) -> Value {
        let entries = self
            .entries
            .iter()
            .map(NotificationEntry::to_json_value)
            .collect::<Vec<_>>();

        let report = self
            .report
            .as_ref()
            .map(report_to_json_value)
            .unwrap_or(Value::Null);

        json!({
            "report": report,
            "title": self.static_data.title,
            "host": self.static_data.host,
            "entries": entries,
        })
    }

    /// Serialize the payload into a compact JSON string.
    pub fn to_json_string(&self) -> serde_json::Result<String> {
        serde_json::to_string(&self.to_json_value())
    }
}

/// Format the notification title the same way as the legacy notifier.
pub fn get_title(hostname: &str, tag: &str) -> String {
    let mut title = String::new();

    if !tag.is_empty() {
        title.push('[');
        title.push_str(tag);
        title.push_str("] ");
    }

    title.push_str("Watchtower updates");

    if !hostname.is_empty() {
        title.push_str(" on ");
        title.push_str(hostname);
    }

    title
}

/// Resolve the static notification data without depending on CLI parsing.
pub fn get_template_data(input: &TemplateDataInput) -> StaticData {
    let host = input.hostname();
    let title = if input.skip_title {
        String::new()
    } else {
        get_title(&host, &input.title_tag())
    };

    StaticData { title, host }
}

/// Return the legacy delay when present, otherwise the explicitly configured delay.
pub fn get_delay(configured_delay: Option<Duration>, legacy_delay: Duration) -> Duration {
    if legacy_delay > Duration::ZERO {
        legacy_delay
    } else {
        configured_delay
            .filter(|delay| *delay > Duration::ZERO)
            .unwrap_or(Duration::ZERO)
    }
}

fn report_to_json_value(report: &Report) -> Value {
    json!({
        "scanned": reports_to_json_values(&report.scanned),
        "updated": reports_to_json_values(&report.updated),
        "failed": reports_to_json_values(&report.failed),
        "skipped": reports_to_json_values(&report.skipped),
        "stale": reports_to_json_values(&report.stale),
        "fresh": reports_to_json_values(&report.fresh),
    })
}

fn reports_to_json_values(reports: &[ContainerReport]) -> Vec<Value> {
    reports.iter().map(report_entry_to_json_value).collect()
}

fn report_entry_to_json_value(report: &ContainerReport) -> Value {
    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(report.id.short_id()));
    object.insert("name".to_string(), Value::String(report.name.clone()));
    object.insert(
        "currentImageId".to_string(),
        Value::String(report.current_image_id.short_id()),
    );
    object.insert(
        "latestImageId".to_string(),
        Value::String(report.latest_image_id.short_id()),
    );
    object.insert(
        "imageName".to_string(),
        Value::String(report.image_name.clone()),
    );
    object.insert("state".to_string(), Value::String(report.state.clone()));

    if let Some(error) = report.error.as_ref().filter(|error| !error.is_empty()) {
        object.insert("error".to_string(), Value::String(error.clone()));
    }

    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContainerID, ImageID};

    #[test]
    fn get_title_uses_simple_default_without_host_or_tag() {
        assert_eq!(get_title("", ""), "Watchtower updates");
    }

    #[test]
    fn get_title_applies_tag_and_hostname() {
        assert_eq!(
            get_title("test.host", "PREFIX"),
            "[PREFIX] Watchtower updates on test.host"
        );
    }

    #[test]
    fn get_template_data_prefers_explicit_hostname_and_tag() {
        let input = TemplateDataInput {
            configured_hostname: Some("test.host".to_string()),
            fallback_hostname: Some("machine.local".to_string()),
            skip_title: false,
            title_tag: Some("PREFIX".to_string()),
            legacy_email_subject_tag: Some("LEGACY".to_string()),
        };

        assert_eq!(
            get_template_data(&input),
            StaticData {
                title: "[PREFIX] Watchtower updates on test.host".to_string(),
                host: "test.host".to_string(),
            }
        );
    }

    #[test]
    fn get_template_data_uses_legacy_tag_fallback() {
        let input = TemplateDataInput {
            fallback_hostname: Some("machine.local".to_string()),
            legacy_email_subject_tag: Some("LEGACY".to_string()),
            ..TemplateDataInput::default()
        };

        assert_eq!(
            get_template_data(&input),
            StaticData {
                title: "[LEGACY] Watchtower updates on machine.local".to_string(),
                host: "machine.local".to_string(),
            }
        );
    }

    #[test]
    fn get_template_data_can_skip_title() {
        let input = TemplateDataInput {
            configured_hostname: Some("test.host".to_string()),
            skip_title: true,
            title_tag: Some("PREFIX".to_string()),
            ..TemplateDataInput::default()
        };

        assert_eq!(
            get_template_data(&input),
            StaticData {
                title: String::new(),
                host: "test.host".to_string(),
            }
        );
    }

    #[test]
    fn get_delay_prefers_legacy_delay() {
        assert_eq!(
            get_delay(Some(Duration::from_secs(5)), Duration::from_secs(7)),
            Duration::from_secs(7)
        );
    }

    #[test]
    fn get_delay_uses_configured_delay_when_no_legacy_delay_exists() {
        assert_eq!(
            get_delay(Some(Duration::from_secs(5)), Duration::ZERO),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn get_delay_falls_back_to_zero() {
        assert_eq!(get_delay(None, Duration::ZERO), Duration::ZERO);
        assert_eq!(
            get_delay(Some(Duration::ZERO), Duration::ZERO),
            Duration::ZERO
        );
    }

    #[test]
    fn data_json_matches_legacy_shape() {
        let expected = json!({
            "entries": [
                {
                    "data": Value::Null,
                    "level": "info",
                    "message": "foo Bar",
                    "time": "0001-01-01T00:00:00Z"
                }
            ],
            "host": "Mock",
            "report": {
                "failed": [
                    {
                        "currentImageId": "01d210000000",
                        "error": "accidentally the whole container",
                        "id": "c79210000000",
                        "imageName": "mock/fail1:latest",
                        "latestImageId": "d0a210000000",
                        "name": "fail1",
                        "state": "Failed"
                    }
                ],
                "fresh": [
                    {
                        "currentImageId": "01d310000000",
                        "id": "c79310000000",
                        "imageName": "mock/frsh1:latest",
                        "latestImageId": "01d310000000",
                        "name": "frsh1",
                        "state": "Fresh"
                    }
                ],
                "scanned": [
                    {
                        "currentImageId": "01d110000000",
                        "id": "c79110000000",
                        "imageName": "mock/updt1:latest",
                        "latestImageId": "d0a110000000",
                        "name": "updt1",
                        "state": "Updated"
                    },
                    {
                        "currentImageId": "01d120000000",
                        "id": "c79120000000",
                        "imageName": "mock/updt2:latest",
                        "latestImageId": "d0a120000000",
                        "name": "updt2",
                        "state": "Updated"
                    },
                    {
                        "currentImageId": "01d210000000",
                        "error": "accidentally the whole container",
                        "id": "c79210000000",
                        "imageName": "mock/fail1:latest",
                        "latestImageId": "d0a210000000",
                        "name": "fail1",
                        "state": "Failed"
                    },
                    {
                        "currentImageId": "01d310000000",
                        "id": "c79310000000",
                        "imageName": "mock/frsh1:latest",
                        "latestImageId": "01d310000000",
                        "name": "frsh1",
                        "state": "Fresh"
                    }
                ],
                "skipped": [
                    {
                        "currentImageId": "01d410000000",
                        "error": "unpossible",
                        "id": "c79410000000",
                        "imageName": "mock/skip1:latest",
                        "latestImageId": "01d410000000",
                        "name": "skip1",
                        "state": "Skipped"
                    }
                ],
                "stale": [],
                "updated": [
                    {
                        "currentImageId": "01d110000000",
                        "id": "c79110000000",
                        "imageName": "mock/updt1:latest",
                        "latestImageId": "d0a110000000",
                        "name": "updt1",
                        "state": "Updated"
                    },
                    {
                        "currentImageId": "01d120000000",
                        "id": "c79120000000",
                        "imageName": "mock/updt2:latest",
                        "latestImageId": "d0a120000000",
                        "name": "updt2",
                        "state": "Updated"
                    }
                ]
            },
            "title": "Watchtower updates on Mock"
        });

        let data = Data::new(
            StaticData {
                title: "Watchtower updates on Mock".to_string(),
                host: "Mock".to_string(),
            },
            vec![NotificationEntry::new(
                "info",
                "foo Bar",
                None,
                "0001-01-01T00:00:00Z",
            )],
            Some(Report {
                scanned: vec![
                    report(
                        "c79110000000",
                        "updt1",
                        "01d110000000",
                        "d0a110000000",
                        "mock/updt1:latest",
                        None,
                        "Updated",
                    ),
                    report(
                        "c79120000000",
                        "updt2",
                        "01d120000000",
                        "d0a120000000",
                        "mock/updt2:latest",
                        None,
                        "Updated",
                    ),
                    report(
                        "c79210000000",
                        "fail1",
                        "01d210000000",
                        "d0a210000000",
                        "mock/fail1:latest",
                        Some("accidentally the whole container"),
                        "Failed",
                    ),
                    report(
                        "c79310000000",
                        "frsh1",
                        "01d310000000",
                        "01d310000000",
                        "mock/frsh1:latest",
                        None,
                        "Fresh",
                    ),
                ],
                updated: vec![
                    report(
                        "c79110000000",
                        "updt1",
                        "01d110000000",
                        "d0a110000000",
                        "mock/updt1:latest",
                        None,
                        "Updated",
                    ),
                    report(
                        "c79120000000",
                        "updt2",
                        "01d120000000",
                        "d0a120000000",
                        "mock/updt2:latest",
                        None,
                        "Updated",
                    ),
                ],
                failed: vec![
                    report(
                        "c79210000000",
                        "fail1",
                        "01d210000000",
                        "d0a210000000",
                        "mock/fail1:latest",
                        Some("accidentally the whole container"),
                        "Failed",
                    ),
                ],
                skipped: vec![
                    report(
                        "c79410000000",
                        "skip1",
                        "01d410000000",
                        "01d410000000",
                        "mock/skip1:latest",
                        Some("unpossible"),
                        "Skipped",
                    ),
                ],
                stale: vec![],
                fresh: vec![
                    report(
                        "c79310000000",
                        "frsh1",
                        "01d310000000",
                        "01d310000000",
                        "mock/frsh1:latest",
                        None,
                        "Fresh",
                    ),
                ],
            }),
        );

        assert_eq!(data.to_json_value(), expected);
        assert_eq!(
            serde_json::from_str::<Value>(
                &data
                    .to_json_string()
                    .expect("json serialization should succeed"),
            )
            .expect("serialized json should parse"),
            expected
        );
    }

    #[test]
    fn data_json_uses_null_report_and_preserves_entry_payloads() {
        let data = Data::new(
            StaticData {
                title: "Watchtower updates".to_string(),
                host: "Mock".to_string(),
            },
            vec![NotificationEntry::new(
                "error",
                "update failed",
                Some(json!({"container": "api", "attempt": 2})),
                "2026-06-20T09:30:00Z",
            )],
            None,
        );

        assert_eq!(
            data.to_json_value(),
            json!({
                "entries": [
                    {
                        "data": {
                            "attempt": 2,
                            "container": "api"
                        },
                        "level": "error",
                        "message": "update failed",
                        "time": "2026-06-20T09:30:00Z"
                    }
                ],
                "host": "Mock",
                "report": Value::Null,
                "title": "Watchtower updates"
            })
        );
    }

    fn report(
        id: &str,
        name: &str,
        current_image_id: &str,
        latest_image_id: &str,
        image_name: &str,
        error: Option<&str>,
        state: &str,
    ) -> ContainerReport {
        ContainerReport {
            id: ContainerID::from(id),
            name: name.to_string(),
            current_image_id: ImageID::from(current_image_id),
            latest_image_id: ImageID::from(latest_image_id),
            image_name: image_name.to_string(),
            error: error.map(ToString::to_string),
            state: state.to_string(),
        }
    }
}
