#![forbid(unsafe_code)]

//! Container filtering translated from `old-source/pkg/filters/filters.go`.
//!
//! The functions in this module preserve the Go semantics:
//! - exact and regex-based name matching
//! - disable-name exclusion
//! - enable/disable label gating
//! - scope handling with `none`
//! - image repository matching without tag comparison
//! - the human-readable description string used by the CLI

use regex::Regex;

/// A boxed filter predicate.
pub type Filter<'a, C> = Box<dyn Fn(&C) -> bool + 'a>;

/// Minimal container view required by the legacy filter logic.
pub trait FilterableContainer {
    fn name(&self) -> &str;
    fn is_watchtower(&self) -> bool;
    fn enabled(&self) -> (bool, bool);
    fn scope(&self) -> (Option<&str>, bool);
    fn image_name(&self) -> &str;
}

/// Filters only Watchtower containers.
pub fn watchtower_containers_filter<C: FilterableContainer + ?Sized>(container: &C) -> bool {
    container.is_watchtower()
}

/// Legacy alias for [`watchtower_containers_filter`].
pub fn watchtower_only<C: FilterableContainer + ?Sized>(container: &C) -> bool {
    watchtower_containers_filter(container)
}

/// Accepts all containers.
pub fn no_filter<C: ?Sized>(_: &C) -> bool {
    true
}

/// Legacy alias for [`no_filter`].
pub fn allow_all<C: ?Sized>(container: &C) -> bool {
    no_filter(container)
}

/// Returns all containers that match one of the specified names.
pub fn filter_by_names<'a, C>(names: &'a [String], base_filter: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    if names.is_empty() {
        return base_filter;
    }

    Box::new(move |container| {
        let name = container.name();
        for pattern in names {
            if pattern == name || pattern == name.strip_prefix('/').unwrap_or(name) {
                return base_filter(container);
            }

            if regex_matches_legacy_name(pattern, name) {
                return base_filter(container);
            }
        }

        false
    })
}

/// Returns all containers that do not match any of the specified names.
pub fn filter_by_disable_names<'a, C>(
    disable_names: &'a [String],
    base_filter: Filter<'a, C>,
) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    if disable_names.is_empty() {
        return base_filter;
    }

    Box::new(move |container| {
        let name = container.name();
        for pattern in disable_names {
            if pattern == name || pattern == name.strip_prefix('/').unwrap_or(name) {
                return false;
            }
        }

        base_filter(container)
    })
}

/// Returns all containers that have the enabled label set.
pub fn filter_by_enable_label<'a, C>(base_filter: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    Box::new(move |container| {
        let (_, is_set) = container.enabled();
        if !is_set {
            return false;
        }

        base_filter(container)
    })
}

/// Returns all containers that have the enabled label set to disable.
pub fn filter_by_disabled_label<'a, C>(base_filter: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    Box::new(move |container| {
        let (enabled, is_set) = container.enabled();
        if is_set && !enabled {
            return false;
        }

        base_filter(container)
    })
}

/// Returns all containers that belong to a specific scope.
pub fn filter_by_scope<'a, C>(scope: &'a str, base_filter: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    Box::new(move |container| {
        let (container_scope, has_scope) = container.scope();
        let container_scope = if !has_scope || container_scope.is_none() || container_scope == Some("") {
            "none"
        } else {
            container_scope.unwrap_or("none")
        };

        container_scope == scope && base_filter(container)
    })
}

/// Returns all containers that have a specific image repository.
///
/// The tag is ignored, matching the Go `strings.Split(image, ":")[0]` behavior.
pub fn filter_by_image<'a, C>(
    images: Option<&'a [String]>,
    base_filter: Filter<'a, C>,
) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    match images {
        None => base_filter,
        Some(images) => Box::new(move |container| {
            let image = image_repository(container.image_name());
            for target_image in images {
                if image == target_image {
                    return base_filter(container);
                }
            }

            false
        }),
    }
}

/// Builds the needed filter of containers and the human-readable description.
pub fn build_filter<'a, C>(
    names: &'a [String],
    disable_names: &'a [String],
    enable_label: bool,
    scope: &'a str,
) -> (Filter<'a, C>, String)
where
    C: FilterableContainer + ?Sized + 'a,
{
    let mut filter: Filter<'a, C> = Box::new(no_filter::<C>);
    filter = filter_by_names(names, filter);
    filter = filter_by_disable_names(disable_names, filter);

    if enable_label {
        filter = filter_by_enable_label(filter);
    }

    if scope == "none" || !scope.is_empty() {
        filter = filter_by_scope(scope, filter);
    }

    filter = filter_by_disabled_label(filter);

    (
        filter,
        build_filter_description(names, disable_names, enable_label, scope),
    )
}

/// Build the human-readable filter description used by the CLI.
pub fn build_filter_description(
    names: &[String],
    disable_names: &[String],
    enable_label: bool,
    scope: &str,
) -> String {
    let mut details = String::new();

    if !names.is_empty() {
        details.push_str("which name matches \"");
        for (index, name) in names.iter().enumerate() {
            details.push_str(name);
            if index + 1 < names.len() {
                details.push_str("\" or \"");
            }
        }
        details.push_str("\", ");
    }

    if !disable_names.is_empty() {
        details.push_str("not named one of \"");
        for (index, name) in disable_names.iter().enumerate() {
            details.push_str(name);
            if index + 1 < disable_names.len() {
                details.push_str("\" or \"");
            }
        }
        details.push_str("\", ");
    }

    if enable_label {
        details.push_str("using enable label, ");
    }

    if scope == "none" {
        details.push_str("without a scope, \"");
    } else if !scope.is_empty() {
        details.push_str("in scope \"");
        details.push_str(scope);
        details.push_str("\", ");
    }

    if details.is_empty() {
        "Checking all containers (except explicitly disabled with label)".to_string()
    } else {
        let mut description = String::from("Only checking containers ");
        description.push_str(&details);
        description.truncate(description.len().saturating_sub(2));
        description
    }
}

fn image_repository(image_name: &str) -> &str {
    image_name
        .split_once(':')
        .map(|(image, _)| image)
        .unwrap_or(image_name)
}

fn regex_matches_legacy_name(pattern: &str, name: &str) -> bool {
    let regex = match Regex::new(pattern) {
        Ok(regex) => regex,
        Err(_) => return false,
    };

    if let Some(found) = regex.find(name) {
        if found.start() <= 1 && found.end() >= name.len().saturating_sub(1) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Container {
        name: String,
        watchtower: bool,
        enabled: (bool, bool),
        scope: (Option<String>, bool),
        image: String,
    }

    impl FilterableContainer for Container {
        fn name(&self) -> &str {
            &self.name
        }

        fn is_watchtower(&self) -> bool {
            self.watchtower
        }

        fn enabled(&self) -> (bool, bool) {
            self.enabled
        }

        fn scope(&self) -> (Option<&str>, bool) {
            (self.scope.0.as_deref(), self.scope.1)
        }

        fn image_name(&self) -> &str {
            &self.image
        }
    }

    fn container(name: &str) -> Container {
        Container {
            name: name.to_string(),
            watchtower: false,
            enabled: (false, false),
            scope: (None, false),
            image: String::new(),
        }
    }

    #[test]
    fn watchtower_and_no_filter_match_legacy_behavior() {
        let watchtower = Container {
            watchtower: true,
            ..container("watchtower")
        };
        let regular = container("regular");

        assert!(watchtower_containers_filter(&watchtower));
        assert!(watchtower_only(&watchtower));
        assert!(!watchtower_containers_filter(&regular));
        assert!(no_filter(&regular));
        assert!(allow_all(&regular));
    }

    #[test]
    fn filter_by_names_matches_exact_and_regex_like_patterns() {
        let names = vec!["test".to_string(), "ba(b|ll)oon".to_string()];
        let filter = filter_by_names(&names, Box::new(no_filter::<Container>));

        assert!(filter(&container("test")));
        assert!(filter(&container("/test")));
        assert!(filter(&container("balloon")));
        assert!(!filter(&container("spoon")));
        assert!(!filter(&container("baboonious")));
    }

    #[test]
    fn filter_by_names_keeps_legacy_leading_slash_regex_boundary() {
        let names = vec!["oo$".to_string()];
        let filter = filter_by_names(&names, Box::new(no_filter::<Container>));

        assert!(!filter(&container("/foo")));
    }

    #[test]
    fn filter_by_disable_names_uses_exact_name_matching_only() {
        let names = vec!["excluded".to_string()];
        let filter = filter_by_disable_names(&names, Box::new(no_filter::<Container>));

        assert!(!filter(&container("excluded")));
        assert!(!filter(&container("/excluded")));
        assert!(filter(&container("excludedAsSubstring")));
    }

    #[test]
    fn label_filters_follow_legacy_rules() {
        let enable_filter = filter_by_enable_label(Box::new(no_filter::<Container>));
        let disabled_filter = filter_by_disabled_label(Box::new(no_filter::<Container>));

        let enabled_true = Container {
            enabled: (true, true),
            ..container("named")
        };
        let enabled_false = Container {
            enabled: (false, true),
            ..container("named")
        };
        let label_absent = Container {
            enabled: (false, false),
            ..container("named")
        };

        assert!(enable_filter(&enabled_true));
        assert!(enable_filter(&enabled_false));
        assert!(!enable_filter(&label_absent));

        assert!(disabled_filter(&enabled_true));
        assert!(!disabled_filter(&enabled_false));
        assert!(disabled_filter(&label_absent));
    }

    #[test]
    fn filter_by_scope_treats_missing_and_empty_scope_as_none() {
        let filter = filter_by_scope("none", Box::new(no_filter::<Container>));

        let missing = Container {
            scope: (None, false),
            ..container("missing")
        };
        let empty = Container {
            scope: (Some(String::new()), true),
            ..container("empty")
        };
        let explicit_none = Container {
            scope: (Some("none".to_string()), true),
            ..container("none")
        };
        let other = Container {
            scope: (Some("team".to_string()), true),
            ..container("team")
        };

        assert!(filter(&missing));
        assert!(filter(&empty));
        assert!(filter(&explicit_none));
        assert!(!filter(&other));
    }

    #[test]
    fn filter_by_image_ignores_tags_and_preserves_base_filter() {
        let images = vec!["registry".to_string(), "other".to_string()];
        let filter = filter_by_image(Some(&images), Box::new(no_filter::<Container>));
        let no_images = filter_by_image(None, Box::new(no_filter::<Container>));

        let registry_tagged = Container {
            image: "registry:latest".to_string(),
            ..container("registry")
        };
        let digest = Container {
            image: "registry@sha256:deadbeef".to_string(),
            ..container("digest")
        };
        let mismatch = Container {
            image: "example:latest".to_string(),
            ..container("example")
        };

        assert!(filter(&registry_tagged));
        assert!(!filter(&digest));
        assert!(!filter(&mismatch));
        assert!(no_images(&mismatch));
    }

    #[test]
    fn build_filter_matches_legacy_composition_and_description() {
        let names = vec!["test".to_string(), "valid".to_string()];
        let (filter, desc) = build_filter::<Container>(&names, &[], false, "");

        assert!(desc.contains("which name matches"));
        assert!(desc.contains("test"));
        assert!(desc.contains("or"));
        assert!(desc.contains("valid"));

        let invalid = Container {
            enabled: (false, false),
            ..container("Invalid")
        };
        let matching = Container {
            enabled: (true, true),
            ..container("test")
        };

        assert!(!filter(&invalid));
        assert!(filter(&matching));
    }

    #[test]
    fn build_filter_enable_label_and_none_scope_match_legacy_behavior() {
        let names = vec!["test".to_string()];
        let (filter, desc) = build_filter::<Container>(&names, &[], true, "none");

        assert!(desc.contains("using enable label"));
        assert!(desc.contains("without a scope"));

        let scoped = Container {
            enabled: (false, false),
            scope: (Some("anyscope".to_string()), true),
            ..container("scoped")
        };
        let empty_scope = Container {
            enabled: (false, false),
            scope: (Some(String::new()), true),
            ..container("empty")
        };
        let unscoped = Container {
            enabled: (false, false),
            scope: (None, false),
            ..container("unscoped")
        };

        assert!(!filter(&scoped));
        assert!(!filter(&empty_scope));
        assert!(!filter(&unscoped));
    }
}
