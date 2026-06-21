#![forbid(unsafe_code)]

//! UpdateParams struct controlling update pass behaviour.
//!
//! Translated from `old-source/pkg/types/update_params.go`.

use std::fmt;
use std::sync::Arc;

use super::filter::Filter;
use super::filterable_container::FilterableContainer;

/// Parameters that control an update pass.
#[derive(Clone, Default)]
pub struct UpdateParams {
    pub filter: Option<Arc<Filter>>,
    pub cleanup: bool,
    pub no_restart: bool,
    pub timeout: std::time::Duration,
    pub monitor_only: bool,
    pub no_pull: bool,
    pub lifecycle_hooks: bool,
    pub rolling_restart: bool,
    pub label_precedence: bool,
}

impl UpdateParams {
    /// Build a new parameter set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a filter predicate.
    pub fn with_filter(
        mut self,
        filter: impl Fn(&dyn FilterableContainer) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.filter = Some(Arc::new(filter));
        self
    }

    /// Return true when the container passes the configured filter.
    pub fn matches(&self, container: &dyn FilterableContainer) -> bool {
        self.filter.as_ref().is_none_or(|filter| filter(container))
    }
}

impl fmt::Debug for UpdateParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateParams")
            .field("filter", &self.filter.as_ref().map(|_| "<filter>"))
            .field("cleanup", &self.cleanup)
            .field("no_restart", &self.no_restart)
            .field("timeout", &self.timeout)
            .field("monitor_only", &self.monitor_only)
            .field("no_pull", &self.no_pull)
            .field("lifecycle_hooks", &self.lifecycle_hooks)
            .field("rolling_restart", &self.rolling_restart)
            .field("label_precedence", &self.label_precedence)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::filterable_container::FilterableContainer;

    struct MockContainer {
        name: String,
        watchtower: bool,
        enabled: (bool, bool),
        scope: Option<String>,
        image_name: String,
    }

    impl FilterableContainer for MockContainer {
        fn name(&self) -> &str {
            &self.name
        }

        fn is_watchtower(&self) -> bool {
            self.watchtower
        }

        fn enabled(&self) -> (bool, bool) {
            self.enabled
        }

        fn scope(&self) -> Option<&str> {
            self.scope.as_deref()
        }

        fn image_name(&self) -> &str {
            &self.image_name
        }
    }

    #[test]
    fn update_params_matches_filter() {
        let params = UpdateParams::new().with_filter(|container| {
            container.name() == "watchtower" && !container.is_watchtower()
        });
        let container = MockContainer {
            name: "watchtower".to_string(),
            watchtower: false,
            enabled: (true, true),
            scope: Some("default".to_string()),
            image_name: "containrrr/watchtower:latest".to_string(),
        };

        assert!(params.matches(&container));
        assert_eq!(container.enabled(), (true, true));
        assert_eq!(container.scope(), Some("default"));
        assert_eq!(container.image_name(), "containrrr/watchtower:latest");
    }
}
