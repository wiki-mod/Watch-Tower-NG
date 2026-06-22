#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Container metadata and label helpers translated from the legacy Go container package.
//!
//! This module defines the label keys and metadata query helpers that containers
//! expose through their Docker configuration labels. The helpers provide safe access
//! to these labels and parse their values into Rust types.

// Label key constants matching the legacy watchtower metadata schema.
const WATCHTOWER_LABEL: &str = "com.centurylinklabs.watchtower";
const SIGNAL_LABEL: &str = "com.centurylinklabs.watchtower.stop-signal";
const ENABLE_LABEL: &str = "com.centurylinklabs.watchtower.enable";
const MONITOR_ONLY_LABEL: &str = "com.centurylinklabs.watchtower.monitor-only";
const NO_PULL_LABEL: &str = "com.centurylinklabs.watchtower.no-pull";
const DEPENDS_ON_LABEL: &str = "com.centurylinklabs.watchtower.depends-on";
const ZODIAC_LABEL: &str = "com.centurylinklabs.zodiac.original-image";
const SCOPE_LABEL: &str = "com.centurylinklabs.watchtower.scope";
const PRE_CHECK_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-check";
const POST_CHECK_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.post-check";
const PRE_UPDATE_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.pre-update";
const POST_UPDATE_LABEL: &str = "com.centurylinklabs.watchtower.lifecycle.post-update";
const PRE_UPDATE_TIMEOUT_LABEL: &str =
    "com.centurylinklabs.watchtower.lifecycle.pre-update-timeout";
const POST_UPDATE_TIMEOUT_LABEL: &str =
    "com.centurylinklabs.watchtower.lifecycle.post-update-timeout";

/// Error returned by label boolean lookups.
/// Mirrors Go's `errorLabelNotFound` sentinel from `old-source/pkg/container/errors.go`.
#[derive(Debug, PartialEq)]
pub enum LabelError {
    /// The requested label was not present in the container metadata.
    NotFound,
    /// The label was present but its value could not be parsed as a boolean.
    ParseBool(String),
}

impl std::fmt::Display for LabelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("label was not found in container"),
            Self::ParseBool(val) => {
                write!(f, "strconv.ParseBool: parsing {val:?}: invalid syntax")
            }
        }
    }
}

impl std::error::Error for LabelError {}

/// Check whether a label map contains a valid watchtower instance label.
///
/// A valid watchtower label must be present with the value "true".
pub fn contains_watchtower_label(labels: &std::collections::BTreeMap<String, String>) -> bool {
    labels
        .get(WATCHTOWER_LABEL)
        .is_some_and(|value| value == "true")
}

/// Return the pre-check lifecycle command from the label map, or empty string.
/// Mirrors Go's `Container.GetLifecyclePreCheckCommand`.
pub fn get_lifecycle_pre_check_command(
    labels: &std::collections::BTreeMap<String, String>,
) -> String {
    get_label_value_or_empty(labels, PRE_CHECK_LABEL).to_string()
}

/// Return the post-check lifecycle command from the label map, or empty string.
/// Mirrors Go's `Container.GetLifecyclePostCheckCommand`.
pub fn get_lifecycle_post_check_command(
    labels: &std::collections::BTreeMap<String, String>,
) -> String {
    get_label_value_or_empty(labels, POST_CHECK_LABEL).to_string()
}

/// Return the pre-update lifecycle command from the label map, or empty string.
/// Mirrors Go's `Container.GetLifecyclePreUpdateCommand`.
pub fn get_lifecycle_pre_update_command(
    labels: &std::collections::BTreeMap<String, String>,
) -> String {
    get_label_value_or_empty(labels, PRE_UPDATE_LABEL).to_string()
}

/// Return the post-update lifecycle command from the label map, or empty string.
/// Mirrors Go's `Container.GetLifecyclePostUpdateCommand`.
pub fn get_lifecycle_post_update_command(
    labels: &std::collections::BTreeMap<String, String>,
) -> String {
    get_label_value_or_empty(labels, POST_UPDATE_LABEL).to_string()
}

/// Return the label value or empty string.
/// Mirrors Go's `Container.getLabelValueOrEmpty`.
fn get_label_value_or_empty<'a>(
    labels: &'a std::collections::BTreeMap<String, String>,
    label: &str,
) -> &'a str {
    labels.get(label).map(|s| s.as_str()).unwrap_or("")
}

/// Return the label value as an Option.
/// Mirrors Go's `Container.getLabelValue`.
pub(crate) fn get_label_value<'a>(
    labels: &'a std::collections::BTreeMap<String, String>,
    label: &str,
) -> Option<&'a str> {
    labels.get(label).map(|s| s.as_str())
}

/// Parse the label as a boolean, mirroring Go's `strconv.ParseBool` semantics.
/// Returns `LabelError::NotFound` if the label is absent.
/// Mirrors Go's `Container.getBoolLabelValue`.
pub(crate) fn get_bool_label_value(
    labels: &std::collections::BTreeMap<String, String>,
    label: &str,
) -> Result<bool, LabelError> {
    match labels.get(label) {
        Some(val) => {
            parse_bool_go_compat(val).ok_or_else(|| LabelError::ParseBool(val.clone()))
        }
        None => Err(LabelError::NotFound),
    }
}

/// Parse a boolean string using Go's `strconv.ParseBool` accepted values.
fn parse_bool_go_compat(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn contains_watchtower_label_returns_true_when_label_is_present_and_true() {
        let mut labels = BTreeMap::new();
        labels.insert(WATCHTOWER_LABEL.to_string(), "true".to_string());

        assert!(contains_watchtower_label(&labels));
    }

    #[test]
    fn contains_watchtower_label_returns_false_when_label_is_present_but_false() {
        let mut labels = BTreeMap::new();
        labels.insert(WATCHTOWER_LABEL.to_string(), "false".to_string());

        assert!(!contains_watchtower_label(&labels));
    }

    #[test]
    fn contains_watchtower_label_returns_false_when_label_is_missing() {
        let labels = BTreeMap::new();

        assert!(!contains_watchtower_label(&labels));
    }
}
