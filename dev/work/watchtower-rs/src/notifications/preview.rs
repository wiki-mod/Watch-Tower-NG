//! Template preview support for notification rendering.
//!
//! The legacy Go project exposed this behavior from
//! `pkg/notifications/preview`. The Rust rewrite keeps the same deterministic
//! sample data and template evaluator so previewed templates behave like the
//! original tool.

#[path = "../bin/tplprev.rs"]
mod tplprev;

/// Render a notification template with deterministic preview data.
///
/// The `states` and `entries` arguments use the same compact character
/// encoding as the legacy `tplprev` CLI:
/// - `states`: `c`, `u`, `e`, `k`, `t`, `f`
/// - `entries`: `p`, `f`, `e`, `w`, `i`, `d`, `t`
pub fn render(input: &str, states: &str, entries: &str) -> Result<String, String> {
    tplprev::render_preview_from_strings(input, states, entries)
}
