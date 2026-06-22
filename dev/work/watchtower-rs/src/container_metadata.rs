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

/// Check whether a label map contains a valid watchtower instance label.
///
/// A valid watchtower label must be present with the value "true".
pub fn contains_watchtower_label(labels: &std::collections::BTreeMap<String, String>) -> bool {
    labels
        .get(WATCHTOWER_LABEL)
        .is_some_and(|value| value == "true")
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
