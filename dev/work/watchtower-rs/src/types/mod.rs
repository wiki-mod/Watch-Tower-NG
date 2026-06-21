#![forbid(unsafe_code)]

//! Shared legacy watchtower types translated from `old-source/pkg/types/`.

pub mod container;
pub mod convertible_notifier;
pub mod filter;
pub mod filterable_container;
pub mod notifier;
pub mod registry_credentials;
pub mod report;
pub mod token_response;
pub mod update_params;

pub use container::{Container, ContainerID, ImageID, RuntimeContainer};
pub use convertible_notifier::{ConvertibleNotifier, DelayNotifier};
pub use filter::Filter;
pub use filterable_container::FilterableContainer;
pub use notifier::Notifier;
pub use registry_credentials::RegistryCredentials;
pub use report::{ContainerReport, Report};
pub use token_response::TokenResponse;
pub use update_params::UpdateParams;
