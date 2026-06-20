#![forbid(unsafe_code)]

use watchtower_rs::filters::{
    build_filter, filter_by_disabled_label, filter_by_enable_label, filter_by_image,
    filter_by_names, filter_by_scope, no_filter, watchtower_containers_filter,
    FilterableContainer as FilterableContainerTrait,
};

#[derive(Default)]
struct MockContainer {
    enabled: (bool, bool),
    is_watchtower: bool,
    name: String,
    scope: (String, bool),
    image_name: String,
}

impl FilterableContainerTrait for MockContainer {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_watchtower(&self) -> bool {
        self.is_watchtower
    }

    fn enabled(&self) -> (bool, bool) {
        self.enabled
    }

    fn scope(&self) -> (Option<&str>, bool) {
        (Some(self.scope.0.as_str()), self.scope.1)
    }

    fn image_name(&self) -> &str {
        &self.image_name
    }
}

fn mock_container() -> MockContainer {
    MockContainer::default()
}

#[test]
fn test_watchtower_containers_filter() {
    let mut container = mock_container();
    container.is_watchtower = true;

    assert!(watchtower_containers_filter(&container));
}

#[test]
fn test_no_filter() {
    let container = mock_container();

    assert!(no_filter(&container));
}

#[test]
fn test_filter_by_names() {
    let names: Vec<String> = Vec::new();

    let filter = filter_by_names(&names, Box::new(no_filter::<MockContainer>));
    let container = mock_container();
    assert!(filter(&container));

    let names = vec!["test".to_string()];
    let filter = filter_by_names(&names, Box::new(no_filter::<MockContainer>));

    let mut container = mock_container();
    container.name = "test".to_string();
    assert!(filter(&container));

    let mut container = mock_container();
    container.name = "NoTest".to_string();
    assert!(!filter(&container));
}

#[test]
fn test_filter_by_names_regex() {
    let names = vec![String::from(r"ba(b|ll)oon")];

    let filter = filter_by_names(&names, Box::new(no_filter::<MockContainer>));

    let mut container = mock_container();
    container.name = "balloon".to_string();
    assert!(filter(&container));

    let mut container = mock_container();
    container.name = "spoon".to_string();
    assert!(!filter(&container));

    let mut container = mock_container();
    container.name = "baboonious".to_string();
    assert!(!filter(&container));
}

#[test]
fn test_filter_by_enable_label() {
    let filter = filter_by_enable_label(Box::new(no_filter::<MockContainer>));

    let mut container = mock_container();
    container.enabled = (true, true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.enabled = (false, true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.enabled = (false, false);
    assert!(!filter(&container));
}

#[test]
fn test_filter_by_scope() {
    let scope = "testscope";

    let filter = filter_by_scope(scope, Box::new(no_filter::<MockContainer>));

    let mut container = mock_container();
    container.scope = ("testscope".to_string(), true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.scope = ("nottestscope".to_string(), true);
    assert!(!filter(&container));

    let mut container = mock_container();
    container.scope = ("".to_string(), false);
    assert!(!filter(&container));
}

#[test]
fn test_filter_by_none_scope() {
    let scope = "none";

    let filter = filter_by_scope(scope, Box::new(no_filter::<MockContainer>));

    let mut container = mock_container();
    container.scope = ("anyscope".to_string(), true);
    assert!(!filter(&container));

    let mut container = mock_container();
    container.scope = ("".to_string(), false);
    assert!(filter(&container));

    let mut container = mock_container();
    container.scope = ("".to_string(), true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.scope = ("none".to_string(), true);
    assert!(filter(&container));
}

#[test]
fn test_build_filter_none_scope() {
    let (filter, desc) = build_filter::<MockContainer>(&[], &[], false, "none");

    assert!(desc.contains("without a scope"));

    let mut scoped = mock_container();
    scoped.enabled = (false, false);
    scoped.scope = ("anyscope".to_string(), true);

    let mut unscoped = mock_container();
    unscoped.enabled = (false, false);
    unscoped.scope = ("".to_string(), false);

    assert!(!filter(&scoped));
    assert!(filter(&unscoped));
}

#[test]
fn test_filter_by_disabled_label() {
    let filter = filter_by_disabled_label(Box::new(no_filter::<MockContainer>));

    let mut container = mock_container();
    container.enabled = (true, true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.enabled = (false, true);
    assert!(!filter(&container));

    let mut container = mock_container();
    container.enabled = (false, false);
    assert!(filter(&container));
}

#[test]
fn test_filter_by_image() {
    let filter_empty = filter_by_image(None, Box::new(no_filter::<MockContainer>));
    let images_single = vec!["registry".to_string()];
    let filter_single = filter_by_image(Some(&images_single), Box::new(no_filter::<MockContainer>));
    let images_multiple = vec!["registry".to_string(), "bla".to_string()];
    let filter_multiple =
        filter_by_image(Some(&images_multiple), Box::new(no_filter::<MockContainer>));

    let mut container = mock_container();
    container.image_name = "registry:2".to_string();
    assert!(filter_empty(&container));
    assert!(filter_single(&container));
    assert!(filter_multiple(&container));

    let mut container = mock_container();
    container.image_name = "registry:latest".to_string();
    assert!(filter_empty(&container));
    assert!(filter_single(&container));
    assert!(filter_multiple(&container));

    let mut container = mock_container();
    container.image_name = "abcdef1234".to_string();
    assert!(filter_empty(&container));
    assert!(!filter_single(&container));
    assert!(!filter_multiple(&container));

    let mut container = mock_container();
    container.image_name = "bla:latest".to_string();
    assert!(filter_empty(&container));
    assert!(!filter_single(&container));
    assert!(filter_multiple(&container));
}

#[test]
fn test_build_filter() {
    let names = vec!["test".to_string(), "valid".to_string()];
    let (filter, desc) = build_filter::<MockContainer>(&names, &[], true, "");

    assert!(desc.contains("which name matches"));
    assert!(desc.contains("test"));
    assert!(desc.contains("or"));
    assert!(desc.contains("valid"));
    assert!(desc.contains("using enable label"));

    let mut container = mock_container();
    container.enabled = (false, false);
    assert!(!filter(&container));

    let mut container = mock_container();
    container.name = "Invalid".to_string();
    container.enabled = (true, true);
    assert!(!filter(&container));

    let mut container = mock_container();
    container.name = "test".to_string();
    container.enabled = (true, true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.enabled = (false, true);
    assert!(!filter(&container));
}

#[test]
fn test_build_filter_disable_container() {
    let disable_names = vec!["excluded".to_string(), "notfound".to_string()];
    let (filter, desc) = build_filter::<MockContainer>(&[], &disable_names, false, "");

    assert!(desc.contains("not named"));
    assert!(desc.contains("excluded"));
    assert!(desc.contains("or"));
    assert!(desc.contains("notfound"));

    let mut container = mock_container();
    container.name = "Another".to_string();
    container.enabled = (false, false);
    assert!(filter(&container));

    let mut container = mock_container();
    container.name = "AnotherOne".to_string();
    container.enabled = (true, true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.name = "test".to_string();
    container.enabled = (false, false);
    assert!(filter(&container));

    let mut container = mock_container();
    container.name = "excluded".to_string();
    container.enabled = (true, true);
    assert!(!filter(&container));

    let mut container = mock_container();
    container.name = "excludedAsSubstring".to_string();
    container.enabled = (true, true);
    assert!(filter(&container));

    let mut container = mock_container();
    container.name = "notfound".to_string();
    container.enabled = (true, true);
    assert!(!filter(&container));

    let mut container = mock_container();
    container.enabled = (false, true);
    assert!(!filter(&container));
}
