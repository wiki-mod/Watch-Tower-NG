#![forbid(unsafe_code)]

pub mod check;
pub mod update;

pub use check::{
    build_watchtower_instance_cleanup_plan, check_for_multiple_watchtower_instances,
    check_for_sanity, WatchtowerInstanceCleanupPlan,
};
pub use update::{update, UpdateClient};
