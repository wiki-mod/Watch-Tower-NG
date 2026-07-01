#![forbid(unsafe_code)]

pub mod check;
pub mod update;

pub use check::{check_for_multiple_watchtower_instances, check_for_sanity};
pub use update::{UpdateClient, update};
