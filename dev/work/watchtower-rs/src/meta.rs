#![forbid(unsafe_code)]

const VERSION: &str = "v0.0.0-unknown";
const USER_AGENT: &str = "Watchtower/v0.0.0-unknown";

pub fn version() -> &'static str {
    VERSION
}

pub fn user_agent() -> &'static str {
    USER_AGENT
}
