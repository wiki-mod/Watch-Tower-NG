#![forbid(unsafe_code)]
#![allow(dead_code)]

//! Mock client for testing actions.
//!
//! Translated from `old-source/internal/actions/mocks/client.go`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::Duration;

use watchtower_rs::container::Container;
use watchtower_rs::lifecycle::LifecycleClient;
use watchtower_rs::types::{ContainerID, ImageID, UpdateParams};
use watchtower_rs::actions::UpdateClient;

/// TestData is the data used to perform the test.
///
/// Translated from Go's `TestData` struct.
#[derive(Clone)]
pub struct TestData {
    pub tried_to_remove_image_count: Cell<usize>,
    pub name_of_container_to_keep: String,
    pub containers: Vec<Container>,
    pub staleness: BTreeMap<String, bool>,
}

impl TestData {
    /// Creates a new test data instance.
    pub fn new(
        name_of_container_to_keep: impl Into<String>,
        containers: Vec<Container>,
    ) -> Self {
        Self {
            tried_to_remove_image_count: Cell::new(0),
            name_of_container_to_keep: name_of_container_to_keep.into(),
            containers,
            staleness: BTreeMap::new(),
        }
    }

    /// TriedToRemoveImage is a test helper function to check whether RemoveImageByID has been called.
    ///
    /// Translated from Go's `TriedToRemoveImage()` method.
    pub fn tried_to_remove_image(&self) -> bool {
        self.tried_to_remove_image_count.get() > 0
    }
}

/// MockClient is a mock that passes as a watchtower Client.
///
/// Translated from Go's `MockClient` struct.
pub struct MockClient {
    pub test_data: TestData,
}

impl MockClient {
    /// CreateMockClient creates a mock watchtower Client for usage in tests.
    ///
    /// Translated from Go's `CreateMockClient` function.
    pub fn new(data: TestData) -> Self {
        Self { test_data: data }
    }
}

impl LifecycleClient for MockClient {
    type Error = String;

    /// ListContainers is a mock method returning the provided container testdata.
    fn list_containers(&self) -> Result<Vec<Container>, Self::Error> {
        Ok(self.test_data.containers.clone())
    }

    /// GetContainer is a mock method.
    fn get_container(
        &self,
        _container_id: &ContainerID,
    ) -> Result<Container, Self::Error> {
        self.test_data
            .containers
            .first()
            .cloned()
            .ok_or_else(|| "not used".to_string())
    }

    /// ExecuteCommand is a mock method.
    fn execute_command(
        &self,
        _container_id: &ContainerID,
        command: &str,
        _timeout_minutes: i64,
    ) -> Result<bool, Self::Error> {
        match command {
            "/PreUpdateReturn0.sh" => Ok(false),
            "/PreUpdateReturn1.sh" => Err("command exited with code 1".to_string()),
            "/PreUpdateReturn75.sh" => Ok(true),
            _ => Ok(false),
        }
    }
}

impl UpdateClient for MockClient {
    /// StopContainer is a mock method.
    fn stop_container(
        &self,
        container: &Container,
        _timeout: Duration,
    ) -> Result<(), Self::Error> {
        if container.name() == self.test_data.name_of_container_to_keep {
            return Err("tried to stop the instance we want to keep".to_string());
        }
        Ok(())
    }

    /// StartContainer is a mock method.
    fn start_container(&self, container: &Container) -> Result<ContainerID, Self::Error> {
        Ok(container.id().clone())
    }

    /// RenameContainer is a mock method.
    fn rename_container(
        &self,
        _container: &Container,
        _new_name: &str,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    /// RemoveImageByID increments the TriedToRemoveImageCount on being called.
    fn remove_image_by_id(&self, _image_id: &ImageID) -> Result<(), Self::Error> {
        self.test_data
            .tried_to_remove_image_count
            .set(self.test_data.tried_to_remove_image_count.get() + 1);
        Ok(())
    }

    /// IsContainerStale is true if not explicitly stated in TestData for the mock client.
    fn is_container_stale(
        &self,
        container: &Container,
        _params: &UpdateParams,
    ) -> Result<(bool, ImageID), Self::Error> {
        let stale = self
            .test_data
            .staleness
            .get(container.name())
            .copied()
            .unwrap_or(true);

        if stale {
            Ok((true, ImageID::new("")))
        } else {
            Ok((false, container.image_id().clone()))
        }
    }
}
