#![forbid(unsafe_code)]

use serde::ser::Serializer;
use serde::Serialize;

use super::model::Data;

impl Serialize for Data {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json_value().serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::model::{NotificationEntry, StaticData};
    use serde_json::json;

    #[test]
    fn data_serialize_matches_to_json_value() {
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

        assert_eq!(
            serde_json::to_value(&data).expect("data should serialize"),
            data.to_json_value()
        );
    }
}
