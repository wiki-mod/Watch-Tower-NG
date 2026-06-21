#![forbid(unsafe_code)]

/// Version is the compile-time set version of Watchtower.
/// Can be overridden at build time via WATCHTOWER_VERSION environment variable.
pub const VERSION: &str = env!("WATCHTOWER_VERSION");

/// version returns the Watchtower version string.
pub fn version() -> &'static str {
    VERSION
}

/// user_agent returns the HTTP client identifier derived from Version.
pub fn user_agent() -> String {
    format!("Watchtower/{}", VERSION)
}
