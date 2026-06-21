#![forbid(unsafe_code)]

use serde::Serialize;
use titlecase::titlecase;

/// Legacy template helper equivalent to `strings.ToUpper`.
pub fn template_to_upper(value: &str) -> String {
    value.to_uppercase()
}

/// Legacy template helper equivalent to `strings.ToLower`.
pub fn template_to_lower(value: &str) -> String {
    value.to_lowercase()
}

/// Legacy template helper equivalent to `cases.Title(language.AmericanEnglish).String`.
pub fn template_title(value: &str) -> String {
    titlecase(value)
}

/// Legacy template helper equivalent to `json.MarshalIndent(v, "", "  ")`.
pub fn template_to_json<T>(value: &T) -> String
where
    T: Serialize + ?Sized,
{
    serde_json::to_string_pretty(value).unwrap_or_else(|err| {
        format!("failed to marshal JSON in notification template: {err}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::Serializer;
    use serde_json::json;

    #[test]
    fn template_upper_lower_and_title_helpers_match_legacy_behavior() {
        assert_eq!(template_to_upper("Watchtower"), "WATCHTOWER");
        assert_eq!(template_to_lower("Watchtower"), "watchtower");
        assert_eq!(template_title("watchtower updates"), "Watchtower Updates");
    }

    #[test]
    fn template_to_json_matches_legacy_pretty_format() {
        let payload = json!({
            "entries": [
                {
                    "level": "info",
                    "message": "ok"
                }
            ],
            "host": "mock"
        });

        assert_eq!(
            template_to_json(&payload),
            "{\n  \"entries\": [\n    {\n      \"level\": \"info\",\n      \"message\": \"ok\"\n    }\n  ],\n  \"host\": \"mock\"\n}"
        );
    }

    #[test]
    fn template_to_json_returns_legacy_error_message_on_serialize_failure() {
        struct FailingValue;

        impl Serialize for FailingValue {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                Err(serde::ser::Error::custom("boom"))
            }
        }

        assert_eq!(
            template_to_json(&FailingValue),
            "failed to marshal JSON in notification template: boom"
        );
    }
}
