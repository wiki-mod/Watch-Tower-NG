#![forbid(unsafe_code)]

use serde::Serialize;
use titlecase::titlecase;

/// Equivalent to `strings.ToUpper` in the Go template function map.
/// Returns the uppercase version of the input string.
pub fn template_to_upper(s: &str) -> String {
    s.to_uppercase()
}

/// Equivalent to `strings.ToLower` in the Go template function map.
/// Returns the lowercase version of the input string.
pub fn template_to_lower(s: &str) -> String {
    s.to_lowercase()
}

/// Equivalent to `cases.Title(language.AmericanEnglish).String` in the Go template function map.
/// Returns the title case version of the input string using the titlecase crate.
pub fn template_title(s: &str) -> String {
    titlecase(s)
}

/// Equivalent to the `toJSON` function in the Go template function map.
/// Serializes the input value to a pretty-printed JSON string.
/// If serialization fails, returns an error message in the same format as the Go version.
pub fn template_to_json<T: Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string_pretty(v)
        .unwrap_or_else(|err| format!("failed to marshal JSON in notification template: {}", err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::ser::Serializer;
    use serde_json::json;

    #[test]
    fn test_template_to_upper() {
        assert_eq!(template_to_upper("hello"), "HELLO");
        assert_eq!(template_to_upper("Watchtower"), "WATCHTOWER");
        assert_eq!(template_to_upper(""), "");
    }

    #[test]
    fn test_template_to_lower() {
        assert_eq!(template_to_lower("HELLO"), "hello");
        assert_eq!(template_to_lower("Watchtower"), "watchtower");
        assert_eq!(template_to_lower(""), "");
    }

    #[test]
    fn test_template_title() {
        assert_eq!(template_title("watchtower updates"), "Watchtower Updates");
        assert_eq!(template_title("hello world"), "Hello World");
        assert_eq!(template_title(""), "");
    }

    #[test]
    fn test_template_to_json_pretty_format() {
        let payload = json!({
            "entries": [
                {
                    "level": "info",
                    "message": "ok"
                }
            ],
            "host": "mock"
        });

        let expected = "{\n  \"entries\": [\n    {\n      \"level\": \"info\",\n      \"message\": \"ok\"\n    }\n  ],\n  \"host\": \"mock\"\n}";
        assert_eq!(template_to_json(&payload), expected);
    }

    #[test]
    fn test_template_to_json_error_handling() {
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
