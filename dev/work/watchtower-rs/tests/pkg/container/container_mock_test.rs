#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use watchtower_rs::container::{
    Container, ContainerConfig, ContainerInspect, ContainerState, HealthConfig, HostConfig,
    ImageInspect, PortBinding,
};
use watchtower_rs::types::{ContainerID, ImageID};

pub type MockContainerUpdate = Box<dyn Fn(&mut ContainerInspect, &mut ImageInspect) + 'static>;

pub fn mock_container(updates: impl IntoIterator<Item = MockContainerUpdate>) -> Container {
    let mut container_info = ContainerInspect {
        id: ContainerID::from("container_id"),
        name: "test-containrrr".to_string(),
        image: ImageID::from("image"),
        created: String::new(),
        state: ContainerState::default(),
        config: Some(ContainerConfig {
            labels: BTreeMap::new(),
            ..ContainerConfig::default()
        }),
        host_config: Some(HostConfig::default()),
        network_settings: None,
    };
    let mut image_info = ImageInspect {
        id: ImageID::from("image_id"),
        config: ContainerConfig::default(),
    };

    for update in updates {
        update(&mut container_info, &mut image_info);
    }

    Container::new(container_info, Some(image_info))
}

pub fn with_port_bindings<S>(port_binding_sources: impl IntoIterator<Item = S>) -> MockContainerUpdate
where
    S: Into<String>,
{
    let port_binding_sources = port_binding_sources
        .into_iter()
        .map(Into::into)
        .collect::<Vec<_>>();

    Box::new(move |c: &mut ContainerInspect, _i: &mut ImageInspect| {
        let mut port_bindings = BTreeMap::new();
        for port_binding_source in &port_binding_sources {
            port_bindings.insert(port_binding_source.clone(), Vec::<PortBinding>::new());
        }
        c.host_config.as_mut().unwrap().port_bindings = port_bindings;
    })
}

pub fn with_image_name(name: impl Into<String>) -> MockContainerUpdate {
    let name = name.into();

    Box::new(move |c: &mut ContainerInspect, _i: &mut ImageInspect| {
        c.config.as_mut().unwrap().image = name.clone();
    })
}

pub fn with_links<S>(links: impl IntoIterator<Item = S>) -> MockContainerUpdate
where
    S: Into<String>,
{
    let links = links.into_iter().map(Into::into).collect::<Vec<_>>();

    Box::new(move |c: &mut ContainerInspect, _i: &mut ImageInspect| {
        if c.host_config.is_none() {
            c.host_config = Some(HostConfig::default());
        }
        c.host_config.as_mut().unwrap().links = links.clone();
    })
}

pub fn with_labels(labels: BTreeMap<String, String>) -> MockContainerUpdate {
    Box::new(move |c: &mut ContainerInspect, _i: &mut ImageInspect| {
        c.config.as_mut().unwrap().labels = labels.clone();
    })
}

pub fn with_container_state(state: ContainerState) -> MockContainerUpdate {
    Box::new(move |cnt: &mut ContainerInspect, _img: &mut ImageInspect| {
        cnt.state = state;
    })
}

pub fn with_healthcheck(health_config: HealthConfig) -> MockContainerUpdate {
    Box::new(move |cnt: &mut ContainerInspect, _img: &mut ImageInspect| {
        cnt.config.as_mut().unwrap().healthcheck = Some(health_config.clone());
    })
}

pub fn with_image_healthcheck(health_config: HealthConfig) -> MockContainerUpdate {
    Box::new(move |_cnt: &mut ContainerInspect, img: &mut ImageInspect| {
        img.config.healthcheck = Some(health_config.clone());
    })
}
