#![forbid(unsafe_code)]
#![allow(dead_code, non_snake_case, non_upper_case_globals, non_camel_case_types)]

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::form_urlencoded;

use watchtower_rs::api::{HttpRequest, HttpResponse};
use watchtower_rs::types::{ContainerID, ImageID};

#[path = "container_ref.rs"]
mod container_ref;

pub use container_ref::{ContainerRef, ImageRef};

type Handler = Box<dyn Fn(&HttpRequest) -> HttpResponse + Send + Sync + 'static>;
type Header = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filters(pub BTreeMap<String, BTreeMap<String, bool>>);

impl Filters {
    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0
            .entry(key.into())
            .or_default()
            .insert(value.into(), true);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerInspectResponse {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContainerSummary {
    #[serde(rename = "State")]
    pub state: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageInspectResponse {
    #[serde(rename = "Id")]
    pub id: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeleteResponse {
    #[serde(rename = "Untagged", skip_serializing_if = "Option::is_none")]
    pub untagged: Option<String>,
    #[serde(rename = "Deleted", skip_serializing_if = "Option::is_none")]
    pub deleted: Option<String>,
}

impl DeleteResponse {
    fn untagged(image: impl Into<String>) -> Self {
        Self {
            untagged: Some(image.into()),
            deleted: None,
        }
    }

    fn deleted(image: impl Into<String>) -> Self {
        Self {
            untagged: None,
            deleted: Some(image.into()),
        }
    }
}

fn default_headers() -> Header {
    let mut headers = Header::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    headers
}

fn merge_optional_headers(mut response: HttpResponse, optional_headers: &[Header]) -> HttpResponse {
    for header in optional_headers {
        for (name, value) in header {
            response.headers.insert(name.clone(), value.clone());
        }
    }

    response
}

fn json_response<T: Serialize>(status_code: u16, value: &T, optional_headers: &[Header]) -> HttpResponse {
    let mut response = HttpResponse {
        status: status_code,
        headers: default_headers(),
        body: serde_json::to_string(value).expect("mock JSON encoding should not fail"),
    };

    response = merge_optional_headers(response, optional_headers);
    response
}

fn json_empty_object_response(status_code: u16) -> HttpResponse {
    json_response(status_code, &serde_json::json!({}), &[])
}

fn read_json_file_response(
    relPath: &str,
    statusCode: u16,
    optionalHeader: &[Header],
) -> Result<HttpResponse, String> {
    let buf = getMockJSONFile(relPath)?;
    let body = String::from_utf8(buf)
        .map_err(|err| format!("mock JSON file {relPath:?} is not valid UTF-8: {err}"))?;

    Ok(merge_optional_headers(
        HttpResponse {
            status: statusCode,
            headers: default_headers(),
            body,
        },
        optionalHeader,
    ))
}

fn request_path_and_query(path: &str) -> (&str, &str) {
    match path.split_once('?') {
        Some((route, query)) => (route, query),
        None => (path, ""),
    }
}

fn verify_request(request: &HttpRequest, method: &str, path_suffix: &str, query: Option<&str>) {
    assert_eq!(
        request.method, method,
        "unexpected request method for path {}",
        request.path
    );

    let (path, request_query) = request_path_and_query(&request.path);
    assert!(
        path.ends_with(path_suffix),
        "unexpected request path {}, expected suffix {}",
        path,
        path_suffix
    );

    if let Some(expected_query) = query {
        assert_eq!(
            request_query, expected_query,
            "unexpected request query for path {}",
            request.path
        );
    }
}

fn getMockJSONFile(relPath: &str) -> Result<Vec<u8>, String> {
    let abs_path = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current directory: {err}"))?
        .join(relPath);
    fs::read(&abs_path)
        .map_err(|err| format!("mock JSON file {:?} not found: {err}", abs_path))
}

pub fn RespondWithJSONFile(
    relPath: &str,
    statusCode: u16,
    optionalHeader: &[Header],
) -> Handler {
    let response = respondWithJSONFile(relPath, statusCode, optionalHeader)
        .unwrap_or_else(|err| panic!("{err}"));
    Box::new(move |_| response.clone())
}

fn respondWithJSONFile(
    relPath: &str,
    statusCode: u16,
    optionalHeader: &[Header],
) -> Result<HttpResponse, String> {
    read_json_file_response(relPath, statusCode, optionalHeader)
}

pub fn GetContainerHandlers(containerRefs: &[&ContainerRef]) -> Vec<Handler> {
    let mut handlers = Vec::with_capacity(containerRefs.len() * 3);
    for containerRef in containerRefs {
        handlers.push(getContainerFileHandler(*containerRef));

        for ref_ in &containerRef.references {
            handlers.push(getContainerFileHandler(ref_));
        }

        let image = containerRef
            .image
            .as_ref()
            .expect("container mock requires an image reference");
        handlers.push(getImageHandler(
            image.id.clone(),
            read_json_file_response(&image.get_file_name(), 200, &[])
                .unwrap_or_else(|err| panic!("{err}")),
        ));
    }

    handlers
}

pub fn createFilterArgs(statuses: &[&str]) -> Filters {
    let mut args = Filters::default();
    for status in statuses {
        args.add("status", *status);
    }
    args
}

fn default_image() -> ImageRef {
    ImageRef {
        id: ImageID::new("sha256:4dbc5f9c07028a985e14d1393e849ea07f68804c4293050d5a641b138db72daa"),
        file: "default".to_string(),
    }
}

pub static Watchtower: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    name: "watchtower".to_string(),
    id: ContainerID::new(
        "3d88e0e3543281c747d88b27e246578b65ae8964ba86c7cd7522cf84e0978134",
    ),
    image: Some(Box::new(default_image())),
    file: String::new(),
    references: Vec::new(),
    is_missing: false,
});

pub static Stopped: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    name: "stopped".to_string(),
    id: ContainerID::new(
        "ae8964ba86c7cd7522cf84e09781343d88e0e3543281c747d88b27e246578b65",
    ),
    image: Some(Box::new(default_image())),
    file: String::new(),
    references: Vec::new(),
    is_missing: false,
});

pub static Running: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    name: "running".to_string(),
    id: ContainerID::new(
        "b978af0b858aa8855cce46b628817d4ed58e58f2c4f66c9b9c5449134ed4c008",
    ),
    image: Some(Box::new(ImageRef {
        id: ImageID::new("sha256:19d07168491a3f9e2798a9bed96544e34d57ddc4757a4ac5bb199dea896c87fd"),
        file: "running".to_string(),
    })),
    file: String::new(),
    references: Vec::new(),
    is_missing: false,
});

pub static Restarting: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    name: "restarting".to_string(),
    id: ContainerID::new(
        "ae8964ba86c7cd7522cf84e09781343d88e0e3543281c747d88b27e246578b67",
    ),
    image: Some(Box::new(default_image())),
    file: String::new(),
    references: Vec::new(),
    is_missing: false,
});

static NET_SUPPLIER_OK: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    id: ContainerID::new("25e75393800b5c450a6841212a3b92ed28fa35414a586dec9f2c8a520d4910c2"),
    name: "net_supplier".to_string(),
    image: Some(Box::new(ImageRef {
        id: ImageID::new("sha256:c22b543d33bfdcb9992cbef23961677133cdf09da71d782468ae2517138bad51"),
        file: "net_producer".to_string(),
    })),
    file: String::new(),
    references: Vec::new(),
    is_missing: false,
});

static NET_SUPPLIER_NOT_FOUND: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    id: ContainerID::new(NetSupplierNotFoundID),
    name: (*NET_SUPPLIER_OK).name.clone(),
    image: None,
    file: String::new(),
    references: Vec::new(),
    is_missing: true,
});

pub static NetConsumerOK: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    id: ContainerID::new("1f6b79d2aff23244382026c76f4995851322bed5f9c50631620162f6f9aafbd6"),
    name: "net_consumer".to_string(),
    image: Some(Box::new(ImageRef {
        id: ImageID::new("sha256:904b8cb13b932e23230836850610fa45dce9eb0650d5618c2b1487c2a4f577b8"),
        file: "net_consumer".to_string(),
    })),
    file: String::new(),
    references: vec![(*NET_SUPPLIER_OK).clone()],
    is_missing: false,
});

pub static NetConsumerInvalidSupplier: LazyLock<ContainerRef> = LazyLock::new(|| ContainerRef {
    id: (*NetConsumerOK).id.clone(),
    name: "net_consumer-missing_supplier".to_string(),
    image: (*NetConsumerOK).image.clone(),
    file: String::new(),
    references: vec![(*NET_SUPPLIER_NOT_FOUND).clone()],
    is_missing: false,
});

pub const NetSupplierNotFoundID: &str =
    "badc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc1dbadc";
pub const NetSupplierContainerName: &str = "/wt-contnet-producer-1";

fn getContainerFileHandler(cr: &ContainerRef) -> Handler {
    if cr.is_missing {
        let response = containerNotFoundResponse(cr.id.as_str());
        return Box::new(move |_| response.clone());
    }

    let (containerFile, err) = cr.get_container_file();
    if let Err(err) = err {
        panic!("Failed to get container mock file: {err}");
    }

    let response = read_json_file_response(&containerFile, 200, &[])
        .unwrap_or_else(|err| panic!("{err}"));
    Box::new(move |_| response.clone())
}

fn getContainerHandler(containerId: &str, response: HttpResponse) -> Handler {
    let containerId = containerId.to_string();
    Box::new(move |request| {
        verify_request(
            request,
            "GET",
            &format!("/containers/{containerId}/json"),
            None,
        );
        response.clone()
    })
}

pub fn GetContainerHandler(
    containerID: &str,
    containerInfo: Option<&ContainerInspectResponse>,
) -> Handler {
    let response = match containerInfo {
        Some(containerInfo) => json_response(200, containerInfo, &[]),
        None => containerNotFoundResponse(containerID),
    };

    getContainerHandler(containerID, response)
}

pub fn GetImageHandler(imageInfo: &ImageInspectResponse) -> Handler {
    getImageHandler(ImageID::new(imageInfo.id.clone()), json_response(200, imageInfo, &[]))
}

pub fn ListContainersHandler(statuses: &[&str]) -> Handler {
    let filterArgs = createFilterArgs(statuses);
    let bytes = serde_json::to_string(&filterArgs).expect("mock filter JSON should not fail");
    let query = form_urlencoded::Serializer::new(String::new())
        .append_pair("filters", &bytes)
        .finish();
    let response = respondWithFilteredContainers(&filterArgs);

    Box::new(move |request| {
        verify_request(request, "GET", "containers/json", Some(&query));
        response.clone()
    })
}

fn respondWithFilteredContainers(filters: &Filters) -> HttpResponse {
    let containersJSON =
        getMockJSONFile("./mocks/data/containers.json").expect("mock containers JSON should exist");
    let containers: Vec<ContainerSummary> =
        serde_json::from_slice(&containersJSON).expect("mock containers JSON should be valid");
    let mut filteredContainers = Vec::new();

    for container in containers.iter() {
        for key in filters.0.get("status").into_iter().flat_map(|statuses| statuses.keys()) {
            if container.state == *key {
                filteredContainers.push(container.clone());
            }
        }
    }

    json_response(200, &filteredContainers, &[])
}

fn getImageHandler(imageId: ImageID, response: HttpResponse) -> Handler {
    let imageId = imageId.to_string();
    Box::new(move |request| {
        verify_request(
            request,
            "GET",
            &format!("/images/{imageId}/json"),
            None,
        );
        response.clone()
    })
}

pub fn KillContainerHandler(containerID: &str, found: FoundStatus) -> Handler {
    let response = if found {
        noContentStatusResponse()
    } else {
        containerNotFoundResponse(containerID)
    };

    let containerID = containerID.to_string();
    Box::new(move |request| {
        verify_request(
            request,
            "POST",
            &format!("containers/{containerID}/kill"),
            None,
        );
        response.clone()
    })
}

pub fn RemoveContainerHandler(containerID: &str, found: FoundStatus) -> Handler {
    let response = if found {
        noContentStatusResponse()
    } else {
        containerNotFoundResponse(containerID)
    };

    let containerID = containerID.to_string();
    Box::new(move |request| {
        verify_request(
            request,
            "DELETE",
            &format!("containers/{containerID}"),
            None,
        );
        response.clone()
    })
}

fn containerNotFoundResponse(_containerID: &str) -> HttpResponse {
    json_empty_object_response(404)
}

fn noContentStatusResponse() -> HttpResponse {
    HttpResponse {
        status: 204,
        headers: BTreeMap::new(),
        body: String::new(),
    }
}

pub type FoundStatus = bool;

pub const Found: FoundStatus = true;
pub const Missing: FoundStatus = false;

pub fn RemoveImageHandler(imagesWithParents: &HashMap<String, Vec<String>>) -> Handler {
    let imagesWithParents = imagesWithParents.clone();
    Box::new(move |request| {
        let path = request_path_and_query(&request.path).0;
        assert!(
            path.starts_with("/images/"),
            "unexpected request path {path}, expected /images/..."
        );

        let image = path.rsplit('/').next().unwrap_or_default().to_string();
        if let Some(parents) = imagesWithParents.get(&image) {
            let mut items = vec![
                DeleteResponse::untagged(image.clone()),
                DeleteResponse::deleted(image.clone()),
            ];
            for parent in parents {
                items.push(DeleteResponse::deleted(parent.clone()));
            }
            json_response(200, &items, &[])
        } else {
            json_empty_object_response(404)
        }
    })
}
