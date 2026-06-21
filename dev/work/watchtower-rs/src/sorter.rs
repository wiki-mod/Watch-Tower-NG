#![forbid(unsafe_code)]

//! Sorting helpers translated from the legacy Go sorter package.

use std::cmp::Ordering;
use std::collections::BTreeSet;

#[cfg(not(test))]
use crate::types::RuntimeContainer;
#[cfg(test)]
use crate::types::RuntimeContainer;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Minimal container surface required for dependency sorting.
pub trait SortableContainer {
    fn name(&self) -> &str;
    fn links(&self) -> &[String];
}

#[cfg(not(test))]
impl<T> SortableContainer for T
where
    T: RuntimeContainer,
{
    fn name(&self) -> &str {
        RuntimeContainer::name(self)
    }

    fn links(&self) -> &[String] {
        RuntimeContainer::links(self)
    }
}

/// Sort containers so that dependencies always appear before their dependents.
///
/// The traversal mirrors the legacy Go implementation:
/// - the input order is used as the stable root order
/// - dependency links are followed in the order they appear on each container
/// - links to containers outside the current slice are ignored
pub fn sort_by_dependencies<C>(containers: &[C]) -> Result<Vec<C>, String>
where
    C: SortableContainer + Clone,
{
    DependencySorter::new(containers).sort()
}

/// Sort containers by their creation timestamp, oldest first.
pub fn sort_by_created_at<C>(containers: &[C]) -> Vec<C>
where
    C: RuntimeContainer + Clone,
{
    let mut ordered = containers.to_vec();
    ordered.sort_by(|left, right| compare_created_at(left.created_at(), right.created_at()));
    ordered
}

fn compare_created_at(left: &str, right: &str) -> Ordering {
    let left = OffsetDateTime::parse(left, &Rfc3339).unwrap_or_else(|_| OffsetDateTime::now_utc());
    let right = OffsetDateTime::parse(right, &Rfc3339).unwrap_or_else(|_| OffsetDateTime::now_utc());
    left.cmp(&right)
}

struct DependencySorter<'a, C> {
    containers: &'a [C],
    unvisited: Vec<usize>,
    marked: BTreeSet<String>,
    sorted: Vec<C>,
}

impl<'a, C> DependencySorter<'a, C>
where
    C: SortableContainer + Clone,
{
    fn new(containers: &'a [C]) -> Self {
        Self {
            containers,
            unvisited: (0..containers.len()).collect(),
            marked: BTreeSet::new(),
            sorted: Vec::with_capacity(containers.len()),
        }
    }

    fn sort(mut self) -> Result<Vec<C>, String> {
        while let Some(&index) = self.unvisited.first() {
            self.visit(index)?;
        }

        Ok(self.sorted)
    }

    fn visit(&mut self, index: usize) -> Result<(), String> {
        let container = &self.containers[index];
        let name = container.name().to_string();

        if self.marked.contains(name.as_str()) {
            return Err(format!("circular reference to {name}"));
        }

        self.marked.insert(name.clone());

        for link_name in container.links() {
            if let Some(linked_index) = self.find_unvisited(link_name) {
                self.visit(linked_index)?;
            }
        }

        self.marked.remove(name.as_str());
        self.remove_unvisited(name.as_str());
        self.sorted.push(container.clone());

        Ok(())
    }

    fn find_unvisited(&self, name: &str) -> Option<usize> {
        self.unvisited
            .iter()
            .copied()
            .find(|&index| self.containers[index].name() == name)
    }

    fn remove_unvisited(&mut self, name: &str) {
        if let Some(position) = self
            .unvisited
            .iter()
            .position(|&index| self.containers[index].name() == name)
        {
            self.unvisited.remove(position);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContainerID, ImageID, RuntimeContainer, UpdateParams};

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct MockContainer {
        id: ContainerID,
        name: String,
        links: Vec<String>,
        image_id: ImageID,
        created_at: String,
    }

    impl SortableContainer for MockContainer {
        fn name(&self) -> &str {
            &self.name
        }

        fn links(&self) -> &[String] {
            &self.links
        }
    }

    impl RuntimeContainer for MockContainer {
        fn id(&self) -> &ContainerID {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn links(&self) -> &[String] {
            &self.links
        }

        fn image_id(&self) -> &ImageID {
            &self.image_id
        }

        fn created_at(&self) -> &str {
            &self.created_at
        }

        fn is_watchtower(&self) -> bool {
            false
        }

        fn is_stale(&self) -> bool {
            false
        }

        fn set_stale(&mut self, _value: bool) {}

        fn is_linked_to_restarting(&self) -> bool {
            false
        }

        fn set_linked_to_restarting(&mut self, _value: bool) {}

        fn is_monitor_only(&self, _params: &UpdateParams) -> bool {
            false
        }
    }

    fn container(name: &str, links: &[&str]) -> MockContainer {
        MockContainer {
            id: ContainerID::from(name),
            name: name.to_string(),
            links: links.iter().map(|link| (*link).to_string()).collect(),
            image_id: ImageID::from("sha256:image"),
            created_at: "2024-06-18T12:00:00Z".to_string(),
        }
    }

    fn names(containers: &[MockContainer]) -> Vec<&str> {
        containers
            .iter()
            .map(|container| RuntimeContainer::name(container))
            .collect()
    }

    #[test]
    fn preserves_input_order_without_dependencies() {
        let sorted = sort_by_dependencies(&[
            container("/alpha", &[]),
            container("/beta", &[]),
            container("/gamma", &[]),
        ])
        .expect("sort should succeed");

        assert_eq!(names(&sorted), vec!["/alpha", "/beta", "/gamma"]);
    }

    #[test]
    fn sorts_dependencies_before_dependents() {
        let sorted = sort_by_dependencies(&[
            container("/api", &["/db", "/redis"]),
            container("/db", &[]),
            container("/redis", &[]),
        ])
        .expect("sort should succeed");

        assert_eq!(names(&sorted), vec!["/db", "/redis", "/api"]);
    }

    #[test]
    fn keeps_stable_root_order_across_multiple_trees() {
        let sorted = sort_by_dependencies(&[
            container("/frontend", &["/api"]),
            container("/api", &["/db"]),
            container("/db", &[]),
            container("/worker", &["/queue"]),
            container("/queue", &[]),
        ])
        .expect("sort should succeed");

        assert_eq!(
            names(&sorted),
            vec!["/db", "/api", "/frontend", "/queue", "/worker"]
        );
    }

    #[test]
    fn ignores_links_to_containers_outside_the_slice() {
        let sorted = sort_by_dependencies(&[
            container("/api", &["/db", "/missing"]),
            container("/db", &[]),
        ])
        .expect("sort should succeed");

        assert_eq!(names(&sorted), vec!["/db", "/api"]);
    }

    #[test]
    fn detects_two_node_cycles() {
        let err = sort_by_dependencies(&[
            container("/alpha", &["/beta"]),
            container("/beta", &["/alpha"]),
        ])
        .expect_err("cycle should fail");

        assert_eq!(err, "circular reference to /alpha");
    }

    #[test]
    fn detects_self_cycles() {
        let err = sort_by_dependencies(&[container("/alpha", &["/alpha"])])
            .expect_err("cycle should fail");

        assert_eq!(err, "circular reference to /alpha");
    }

    #[test]
    fn sort_by_created_at_orders_oldest_first() {
        let sorted = sort_by_created_at(&[
            MockContainer {
                created_at: "2024-06-18T12:00:00Z".to_string(),
                ..container("/new", &[])
            },
            MockContainer {
                created_at: "2024-06-18T11:58:00Z".to_string(),
                ..container("/old", &[])
            },
        ]);

        assert_eq!(RuntimeContainer::name(&sorted[0]), "/old");
        assert_eq!(RuntimeContainer::name(&sorted[1]), "/new");
    }
}
