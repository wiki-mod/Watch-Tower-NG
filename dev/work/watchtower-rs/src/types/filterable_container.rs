#![forbid(unsafe_code)]

//! FilterableContainer trait for container filtering surface.
//!
//! Translated from `old-source/pkg/types/filterable_container.go`.

/// Minimal container filter surface used by the update pipeline.
pub trait FilterableContainer {
    fn name(&self) -> &str;
    fn is_watchtower(&self) -> bool;
    fn enabled(&self) -> (bool, bool);
    fn scope(&self) -> Option<&str>;
    fn image_name(&self) -> &str;
}
