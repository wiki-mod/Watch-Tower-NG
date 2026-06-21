#![forbid(unsafe_code)]

pub const VERSION: &str = "v0.0.0-unknown";

pub fn version() -> &'static str {
    VERSION
}

pub fn user_agent() -> String {
    format!("Watchtower/{VERSION}")
}
