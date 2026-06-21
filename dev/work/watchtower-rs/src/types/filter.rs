#![forbid(unsafe_code)]

//! Filter type alias for container filtering.
//!
//! Translated from `old-source/pkg/types/filter.go`.

use super::filterable_container::FilterableContainer;

/// A Filter is a prototype for a function that can be used to filter the
/// results from a call to the `ListContainers()` method on the `Client`.
pub type Filter = dyn Fn(&dyn FilterableContainer) -> bool + Send + Sync;
