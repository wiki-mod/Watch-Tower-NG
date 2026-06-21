#![forbid(unsafe_code)]

use serde::Serialize;
use serde::ser::Serializer;
use serde_json::{Map, Value};

use super::model::Data;
use crate::types::ContainerReport;

/// Implements Serialize for Data to produce JSON in the legacy watchtower shape.
/// This mirrors the behavior of Go's MarshalJSON() method.
impl Serialize for Data {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut root = Map::new();

        // Build entries array: each entry is a map with level, message, data, time
        let entries_array: Vec<Value> = self
            .entries
            .iter()
            .map(|entry| {
                let mut entry_map = Map::new();
                entry_map.insert("level".to_string(), Value::String(entry.level.clone()));
                entry_map.insert("message".to_string(), Value::String(entry.message.clone()));
                entry_map.insert(
                    "data".to_string(),
                    entry.data.clone().unwrap_or(Value::Null),
                );
                entry_map.insert("time".to_string(), Value::String(entry.time.clone()));
                Value::Object(entry_map)
            })
            .collect();

        root.insert("entries".to_string(), Value::Array(entries_array));

        // Build report object or null
        let report_value = match &self.report {
            Some(report) => {
                let mut report_map = Map::new();
                report_map.insert(
                    "scanned".to_string(),
                    Value::Array(marshal_reports(&report.scanned)),
                );
                report_map.insert(
                    "updated".to_string(),
                    Value::Array(marshal_reports(&report.updated)),
                );
                report_map.insert(
                    "failed".to_string(),
                    Value::Array(marshal_reports(&report.failed)),
                );
                report_map.insert(
                    "skipped".to_string(),
                    Value::Array(marshal_reports(&report.skipped)),
                );
                report_map.insert(
                    "stale".to_string(),
                    Value::Array(marshal_reports(&report.stale)),
                );
                report_map.insert(
                    "fresh".to_string(),
                    Value::Array(marshal_reports(&report.fresh)),
                );
                Value::Object(report_map)
            }
            None => Value::Null,
        };

        root.insert("report".to_string(), report_value);
        root.insert(
            "title".to_string(),
            Value::String(self.static_data.title.clone()),
        );
        root.insert(
            "host".to_string(),
            Value::String(self.static_data.host.clone()),
        );

        Value::Object(root).serialize(serializer)
    }
}

/// Private helper function that converts a slice of ContainerReport into an array of JSON values.
/// This mirrors the behavior of Go's marshalReports() function.
fn marshal_reports(reports: &[ContainerReport]) -> Vec<Value> {
    reports
        .iter()
        .map(|report| {
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

            // Only include error field if it is present and non-empty
            if let Some(error) = report.error.as_ref().filter(|e| !e.is_empty()) {
                object.insert("error".to_string(), Value::String(error.clone()));
            }

            Value::Object(object)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::model::{NotificationEntry, StaticData};
    use serde_json::json;

    #[test]
    fn test_serialize_data_matches_to_json_value() {
        let data = Data::new(
            StaticData {
                title: "Watchtower updates on Mock".to_string(),
                host: "Mock".to_string(),
            },
            vec![NotificationEntry::new(
                "info",
                "foo Bar",
                Some(json!({"notify": "yes"})),
                "2026-06-20T15:00:00Z",
            )],
            None,
        );

        // Verify that serialization produces the same JSON shape as to_json_value()
        assert_eq!(
            serde_json::to_value(&data).expect("data should serialize"),
            data.to_json_value()
        );
    }

    #[test]
    fn test_serialize_data_produces_correct_json_shape() {
        let data = Data::new(
            StaticData {
                title: "Test notification".to_string(),
                host: "testhost".to_string(),
            },
            vec![NotificationEntry::new(
                "info",
                "Test message",
                None,
                "2026-06-21T10:00:00Z",
            )],
            None,
        );

        let json_value = serde_json::to_value(&data).expect("should serialize");
        let obj = json_value.as_object().expect("root should be object");

        // Verify required fields are present
        assert!(obj.contains_key("report"));
        assert!(obj.contains_key("title"));
        assert!(obj.contains_key("host"));
        assert!(obj.contains_key("entries"));

        // Verify field values
        assert_eq!(obj["title"], "Test notification");
        assert_eq!(obj["host"], "testhost");
        assert!(obj["entries"].is_array());
    }
}
