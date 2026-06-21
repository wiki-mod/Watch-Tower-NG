#![forbid(unsafe_code)]

//! Template rendering for notification preview.
//!
//! Translated 1:1 from `old-source/pkg/notifications/preview/tplprev.go`.
//!
//! This module provides the core `render()` function which builds a preview data
//! bundle, applies template functions, and executes a notification template.

use std::collections::HashMap;

use go_template::{Context, Func, FuncError, Template, Value};

use super::data::PreviewData;
use super::logs::LogLevel;
use super::report::State;

/// Render a notification template using preview data.
///
/// Maps to the Go signature:
/// ```ignore
/// func Render(input string, states []data.State, loglevels []data.LogLevel) (string, error)
/// ```
#[allow(dead_code)]
pub(crate) fn render(
    input: &str,
    states: &[State],
    log_levels: &[LogLevel],
) -> Result<String, String> {
    let mut data = PreviewData::new();
    let template = build_template(input)?;

    for state in states {
        data.add_from_state(*state);
    }

    for level in log_levels {
        data.add_log_entry(*level);
    }

    template
        .render(&Context::from(data_to_value(&data)))
        .map_err(|err| format!("failed to execute template: {err}"))
}

/// Convert preview data to a go_template Value for template execution.
#[allow(dead_code)]
fn data_to_value(data: &PreviewData) -> Value {
    let mut map = HashMap::new();

    // Convert log entries
    let entries_values: Vec<Value> = data
        .entries
        .iter()
        .map(|entry| {
            let mut entry_map = HashMap::new();
            entry_map.insert("message".to_string(), Value::from(entry.message.clone()));
            entry_map.insert("level".to_string(), Value::from(entry.level.as_str()));
            entry_map.insert("time".to_string(), Value::from(entry.time.to_string()));
            Value::from(entry_map)
        })
        .collect();

    map.insert("Entries".to_string(), Value::from(entries_values));

    // Static data
    let mut static_map = HashMap::new();
    static_map.insert(
        "title".to_string(),
        Value::from(data.static_data.title.clone()),
    );
    static_map.insert(
        "host".to_string(),
        Value::from(data.static_data.host.clone()),
    );
    map.insert("StaticData".to_string(), Value::from(static_map));

    // Report data (if present)
    if let Some(report) = &data.report() {
        let mut report_map = HashMap::new();

        // Convert each report section
        for (name, containers) in &[
            ("scanned", &report.scanned),
            ("updated", &report.updated),
            ("failed", &report.failed),
            ("skipped", &report.skipped),
            ("stale", &report.stale),
            ("fresh", &report.fresh),
        ] {
            let container_values: Vec<Value> = containers
                .iter()
                .map(|c| {
                    let mut c_map = HashMap::new();
                    c_map.insert("id".to_string(), Value::from(c.id.as_str().to_string()));
                    c_map.insert("name".to_string(), Value::from(c.name.clone()));
                    c_map.insert("image_name".to_string(), Value::from(c.image_name.clone()));
                    c_map.insert(
                        "current_image_id".to_string(),
                        Value::from(c.current_image_id.as_str().to_string()),
                    );
                    c_map.insert(
                        "latest_image_id".to_string(),
                        Value::from(c.latest_image_id.as_str().to_string()),
                    );
                    c_map.insert(
                        "error".to_string(),
                        c.error
                            .as_ref()
                            .map(|e| Value::from(e.clone()))
                            .unwrap_or(Value::Nil),
                    );
                    c_map.insert("state".to_string(), Value::from(c.state.as_str()));
                    Value::from(c_map)
                })
                .collect();

            report_map.insert(name.to_string(), Value::from(container_values));
        }

        map.insert("Report".to_string(), Value::from(report_map));
    }

    Value::from(map)
}

/// Build a go_template Template with preview template functions registered.
#[allow(dead_code)]
fn build_template(input: &str) -> Result<Template, String> {
    let mut template = Template::default();
    template.add_funcs(&legacy_funcs());
    template
        .parse(input)
        .map_err(|err| format!("failed to parse {err}"))?;
    Ok(template)
}

/// Legacy template functions available to notification templates.
#[allow(dead_code)]
fn legacy_funcs() -> [(&'static str, Func); 4] {
    [
        ("ToJSON", to_json as Func),
        ("ToUpper", to_upper as Func),
        ("ToLower", to_lower as Func),
        ("Title", to_title as Func),
    ]
}

/// Convert a value to JSON string representation.
#[allow(dead_code)]
fn to_json(args: &[Value]) -> Result<Value, FuncError> {
    if args.len() != 1 {
        return Err(FuncError::ExactlyXArgs("ToJSON".into(), 1));
    }

    match serde_json::to_string_pretty(&serde_json::json!(value_to_json_value(&args[0]))) {
        Ok(json) => Ok(Value::from(json)),
        Err(err) => Ok(Value::from(format!(
            "failed to marshal JSON in notification template: {err}"
        ))),
    }
}

/// Convert go_template Value to serde_json Value for JSON serialization.
#[allow(dead_code)]
fn value_to_json_value(value: &Value) -> serde_json::Value {
    match value {
        Value::NoValue | Value::Nil => serde_json::Value::Null,
        Value::Bool(v) => serde_json::Value::Bool(*v),
        Value::String(v) => serde_json::Value::String(v.clone()),
        Value::Array(v) => {
            let items: Vec<_> = v.iter().map(value_to_json_value).collect();
            serde_json::Value::Array(items)
        }
        Value::Object(v) | Value::Map(v) => {
            let mut out = serde_json::Map::new();
            for (key, item) in v {
                out.insert(key.clone(), value_to_json_value(item));
            }
            serde_json::Value::Object(out)
        }
        Value::Number(v) => {
            if let Some(num) = v.as_i64() {
                serde_json::json!(num)
            } else if let Some(num) = v.as_u64() {
                serde_json::json!(num)
            } else if let Some(num) = v.as_f64() {
                serde_json::json!(num)
            } else {
                serde_json::Value::Null
            }
        }
        Value::Function(_) => serde_json::Value::Null,
    }
}

/// Convert a string value to uppercase.
#[allow(dead_code)]
fn to_upper(args: &[Value]) -> Result<Value, FuncError> {
    map_single_string("ToUpper", args, |value| value.to_uppercase())
}

/// Convert a string value to lowercase.
#[allow(dead_code)]
fn to_lower(args: &[Value]) -> Result<Value, FuncError> {
    map_single_string("ToLower", args, |value| value.to_lowercase())
}

/// Convert a string value to title case.
#[allow(dead_code)]
fn to_title(args: &[Value]) -> Result<Value, FuncError> {
    map_single_string("Title", args, |value| titlecase::titlecase(&value))
}

/// Apply a string transformation to a single string argument.
#[allow(dead_code)]
fn map_single_string(
    name: &'static str,
    args: &[Value],
    map: impl FnOnce(String) -> String,
) -> Result<Value, FuncError> {
    if args.len() != 1 {
        return Err(FuncError::ExactlyXArgs(name.into(), 1));
    }

    let value: String =
        go_template::from_value(&args[0]).ok_or(FuncError::UnableToConvertFromValue)?;
    Ok(Value::from(map(value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_empty_template() {
        let result = render("", &[], &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn render_plain_text() {
        let result = render("Hello World", &[], &[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello World");
    }

    #[test]
    fn render_with_states_and_levels() {
        let states = [State::Updated, State::Failed];
        let levels = [LogLevel::Error, LogLevel::Info];

        let result = render("Static text", &states, &levels);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Static text");
    }

    #[test]
    fn render_invalid_template_fails() {
        let result = render("{{.Unclosed", &[], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn to_upper_func_works() {
        let args = vec![Value::from("hello")];
        let result = to_upper(&args);
        assert!(result.is_ok());
        let val = result.unwrap();
        match val {
            Value::String(s) => assert_eq!(s, "HELLO"),
            _ => panic!("Expected string value"),
        }
    }

    #[test]
    fn to_lower_func_works() {
        let args = vec![Value::from("HELLO")];
        let result = to_lower(&args);
        assert!(result.is_ok());
        let val = result.unwrap();
        match val {
            Value::String(s) => assert_eq!(s, "hello"),
            _ => panic!("Expected string value"),
        }
    }
}
