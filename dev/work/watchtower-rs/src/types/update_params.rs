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
    /// Build a new parameter set with default values.
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

    /// Return true when the container passes the configured filter (or no filter is set).
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
