#![forbid(unsafe_code)]

use std::fs;
use std::io;

use watchtower_rs::types::{ContainerID, ImageID};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRef {
    pub id: ImageID,
    pub file: String,
}

impl ImageRef {
    pub fn get_file_name(&self) -> String {
        format!("./mocks/data/image_{}.json", self.file)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerRef {
    pub name: String,
    pub id: ContainerID,
    pub image: Option<Box<ImageRef>>,
    pub file: String,
    pub references: Vec<ContainerRef>,
    pub is_missing: bool,
}

impl ContainerRef {
    pub fn get_container_file(&self) -> (String, io::Result<fs::Metadata>) {
        let file = if self.file.is_empty() {
            self.name.as_str()
        } else {
            self.file.as_str()
        };

        let container_file = format!("./mocks/data/container_{}.json", file);
        let err = fs::metadata(&container_file);

        (container_file, err)
    }

    pub fn container_id(&self) -> &ContainerID {
        &self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_ref_get_file_name_uses_the_expected_relative_path() {
        let image_ref = ImageRef {
            id: ImageID::new("image-id"),
            file: "example".to_string(),
        };

        assert_eq!(image_ref.get_file_name(), "./mocks/data/image_example.json");
    }

    #[test]
    fn container_ref_get_container_file_uses_file_when_present() {
        let container_ref = ContainerRef {
            name: "name".to_string(),
            id: ContainerID::new("container-id"),
            image: None,
            file: "custom".to_string(),
            references: Vec::new(),
            is_missing: false,
        };

        let (container_file, _) = container_ref.get_container_file();

        assert_eq!(container_file, "./mocks/data/container_custom.json");
    }

    #[test]
    fn container_ref_get_container_file_falls_back_to_name_when_file_is_empty() {
        let container_ref = ContainerRef {
            name: "name".to_string(),
            id: ContainerID::new("container-id"),
            image: None,
            file: String::new(),
            references: Vec::new(),
            is_missing: false,
        };

        let (container_file, _) = container_ref.get_container_file();

        assert_eq!(container_file, "./mocks/data/container_name.json");
    }

    #[test]
    fn container_ref_container_id_returns_the_id() {
        let container_ref = ContainerRef {
            name: "name".to_string(),
            id: ContainerID::new("container-id"),
            image: None,
            file: String::new(),
            references: Vec::new(),
            is_missing: false,
        };

        assert_eq!(container_ref.container_id().as_str(), "container-id");
    }
}
