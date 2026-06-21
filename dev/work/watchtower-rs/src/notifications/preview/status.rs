#![allow(dead_code, non_camel_case_types)]

use crate::types::{ContainerID, ImageID};

use super::report::State;

/// Legacy container snapshot used by the preview report generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct containerStatus {
    container_id: ContainerID,
    old_image: ImageID,
    new_image: ImageID,
    container_name: String,
    image_name: String,
    error: Option<String>,
    state: State,
}

impl containerStatus {
    pub(crate) fn new(
        container_id: ContainerID,
        old_image: ImageID,
        new_image: ImageID,
        container_name: String,
        image_name: String,
        error: Option<String>,
        state: State,
    ) -> Self {
        Self {
            container_id,
            old_image,
            new_image,
            container_name,
            image_name,
            error,
            state,
        }
    }

    pub(crate) fn id(&self) -> &ContainerID {
        &self.container_id
    }

    pub(crate) fn name(&self) -> &str {
        &self.container_name
    }

    pub(crate) fn current_image_id(&self) -> &ImageID {
        &self.old_image
    }

    pub(crate) fn latest_image_id(&self) -> &ImageID {
        &self.new_image
    }

    pub(crate) fn image_name(&self) -> &str {
        &self.image_name
    }

    pub(crate) fn error(&self) -> &str {
        self.error.as_deref().unwrap_or("")
    }

    pub(crate) fn state(&self) -> &str {
        self.state.as_str()
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        ContainerID,
        ImageID,
        ImageID,
        String,
        String,
        Option<String>,
        State,
    ) {
        (
            self.container_id,
            self.old_image,
            self.new_image,
            self.container_name,
            self.image_name,
            self.error,
            self.state,
        )
    }
}
