#![forbid(unsafe_code)]

//! Container selection filters translated from the legacy Watchtower logic.
//!
//! This module intentionally keeps the predicate semantics close to the old Go
//! implementation while staying dependency-free in the Rust slice.

/// A boxed filter predicate over a container-like value.
pub type Filter<'a, C> = Box<dyn Fn(&C) -> bool + 'a>;

/// A minimal container view used by the filter combinators.
///
/// The legacy package relied on a `FilterableContainer` interface. This trait
/// keeps the same data contract while using idiomatic Rust accessors.
pub trait FilterableContainer {
    fn name(&self) -> &str;
    fn is_watchtower(&self) -> bool;
    fn enabled(&self) -> (bool, bool);
    fn scope(&self) -> (Option<&str>, bool);
    fn image_name(&self) -> &str;
}

/// Filters only Watchtower containers.
pub fn watchtower_only<C: FilterableContainer + ?Sized>(container: &C) -> bool {
    container.is_watchtower()
}

/// Accepts all containers.
pub fn allow_all<C: ?Sized>(_: &C) -> bool {
    true
}

/// Compose two predicates with logical `and`.
pub fn compose<'a, C: ?Sized + 'a>(
    base: Filter<'a, C>,
    next: impl Fn(&C) -> bool + 'a,
) -> Filter<'a, C> {
    Box::new(move |container| base(container) && next(container))
}

/// Returns all containers that match one of the specified names.
///
/// Matching follows the legacy behavior:
/// - exact match against the full name;
/// - exact match against the name without a leading slash;
/// - anchored regex-like matching when the pattern is not a literal.
pub fn by_name<'a, C>(names: &'a [String], base: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    if names.is_empty() {
        return base;
    }

    Box::new(move |container| {
        let name = container.name();
        names.iter().any(|pattern| matches_name(pattern, name)) && base(container)
    })
}

/// Returns all containers that do not match any of the specified names.
pub fn by_disable_name<'a, C>(
    disable_names: &'a [String],
    base: Filter<'a, C>,
) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    if disable_names.is_empty() {
        return base;
    }

    Box::new(move |container| {
        let name = container.name();
        if disable_names
            .iter()
            .any(|candidate| candidate == name || candidate == name.strip_prefix('/').unwrap_or(name))
        {
            return false;
        }

        base(container)
    })
}

/// Returns all containers that have the enabled label set.
///
/// The label value does not have to be `true`; the label only needs to be
/// present.
pub fn by_enable_label<'a, C>(base: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    Box::new(move |container| {
        let (_, is_set) = container.enabled();
        is_set && base(container)
    })
}

/// Returns all containers that do not have the enabled label set to disable.
pub fn by_disabled_label<'a, C>(base: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    Box::new(move |container| {
        let (enabled, is_set) = container.enabled();
        if is_set && !enabled {
            return false;
        }

        base(container)
    })
}

/// Returns all containers that belong to a specific scope.
///
/// A missing or empty scope is normalized to `none`, matching the legacy
/// behavior.
pub fn by_scope<'a, C>(scope: &'a str, base: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    Box::new(move |container| {
        let (container_scope, _) = container.scope();
        let container_scope = container_scope.filter(|value| !value.is_empty()).unwrap_or("none");
        container_scope == scope && base(container)
    })
}

/// Returns all containers whose image repository matches one of the targets.
///
/// This keeps the legacy "ignore the tag" behavior by splitting at the first
/// colon.
pub fn by_image<'a, C>(images: Option<&'a [String]>, base: Filter<'a, C>) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    match images {
        None => base,
        Some(images) => Box::new(move |container| {
            let image = image_repository(container.image_name());
            images.iter().any(|candidate| candidate == image) && base(container)
        }),
    }
}

/// Builds the legacy filter chain in the same order as the Go implementation.
///
/// The resulting filter is equivalent to:
/// names -> disable names -> enable label -> scope -> disabled label
pub fn build_filter<'a, C>(
    names: &'a [String],
    disable_names: &'a [String],
    enable_label: bool,
    scope: Option<&'a str>,
) -> Filter<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    let mut filter: Filter<'a, C> = Box::new(allow_all::<C>);
    filter = by_name(names, filter);
    filter = by_disable_name(disable_names, filter);

    if enable_label {
        filter = by_enable_label(filter);
    }

    if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
        filter = by_scope(scope, filter);
    }

    by_disabled_label(filter)
}

/// A small builder for call sites that prefer a fluent style.
pub struct FilterBuilder<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    filter: Filter<'a, C>,
}

impl<'a, C> FilterBuilder<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    pub fn new() -> Self {
        Self {
            filter: Box::new(allow_all::<C>),
        }
    }

    pub fn watchtower_only(self) -> Self {
        self.with_predicate(watchtower_only::<C>)
    }

    pub fn by_name(self, names: &'a [String]) -> Self {
        Self {
            filter: by_name(names, self.filter),
        }
    }

    pub fn by_disable_name(self, disable_names: &'a [String]) -> Self {
        Self {
            filter: by_disable_name(disable_names, self.filter),
        }
    }

    pub fn by_enable_label(self) -> Self {
        Self {
            filter: by_enable_label(self.filter),
        }
    }

    pub fn by_disabled_label(self) -> Self {
        Self {
            filter: by_disabled_label(self.filter),
        }
    }

    pub fn by_scope(self, scope: &'a str) -> Self {
        Self {
            filter: by_scope(scope, self.filter),
        }
    }

    pub fn by_image(self, images: Option<&'a [String]>) -> Self {
        Self {
            filter: by_image(images, self.filter),
        }
    }

    pub fn with_predicate(self, predicate: impl Fn(&C) -> bool + 'a) -> Self {
        Self {
            filter: compose(self.filter, predicate),
        }
    }

    pub fn build(self) -> Filter<'a, C> {
        self.filter
    }
}

impl<'a, C> Default for FilterBuilder<'a, C>
where
    C: FilterableContainer + ?Sized + 'a,
{
    fn default() -> Self {
        Self::new()
    }
}

fn image_repository(image_name: &str) -> &str {
    image_name.split_once(':').map(|(image, _)| image).unwrap_or(image_name)
}

fn matches_name(pattern: &str, name: &str) -> bool {
    if pattern == name {
        return true;
    }

    if pattern == name.strip_prefix('/').unwrap_or(name) {
        return true;
    }

    if let Some(stripped) = name.strip_prefix('/') {
        if regex_like_full_match(pattern, stripped) || regex_like_full_match(pattern, name) {
            return true;
        }
    } else if regex_like_full_match(pattern, name) {
        return true;
    }

    false
}

fn regex_like_full_match(pattern: &str, text: &str) -> bool {
    let mut parser = RegexParser::new(pattern);
    let ast = match parser.parse_expression() {
        Some(ast) if parser.is_eof() => ast,
        _ => return false,
    };

    ast.matches_full(text)
}

#[derive(Debug, Clone)]
enum Expr {
    Empty,
    Literal(String),
    AnyChar,
    Sequence(Vec<Expr>),
    Alternate(Vec<Expr>),
    Repeat { expr: Box<Expr>, kind: RepeatKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepeatKind {
    ZeroOrMore,
    ZeroOrOne,
}

impl Expr {
    fn matches_full(&self, text: &str) -> bool {
        self.match_prefix(text)
            .into_iter()
            .any(|rest| rest.is_empty())
    }

    fn match_prefix<'a>(&self, text: &'a str) -> Vec<&'a str> {
        match self {
            Self::Empty => vec![text],
            Self::Literal(expected) => text
                .strip_prefix(expected.as_str())
                .into_iter()
                .collect(),
            Self::AnyChar => text.chars().next().map_or_else(Vec::new, |ch| vec![&text[ch.len_utf8()..]]),
            Self::Sequence(items) => {
                let mut states = vec![text];
                for item in items {
                    let mut next_states = Vec::new();
                    for state in states {
                        next_states.extend(item.match_prefix(state));
                    }
                    states = next_states;
                    if states.is_empty() {
                        break;
                    }
                }
                states
            }
            Self::Alternate(items) => {
                let mut states = Vec::new();
                for item in items {
                    states.extend(item.match_prefix(text));
                }
                states
            }
            Self::Repeat { expr, kind } => match kind {
                RepeatKind::ZeroOrMore => {
                    let mut states = vec![text];
                    let mut frontier = vec![text];
                    while let Some(state) = frontier.pop() {
                        for next in expr.match_prefix(state) {
                            if next.len() == state.len() {
                                continue;
                            }
                            if !states.contains(&next) {
                                states.push(next);
                                frontier.push(next);
                            }
                        }
                    }
                    states
                }
                RepeatKind::ZeroOrOne => {
                    let mut states = vec![text];
                    states.extend(expr.match_prefix(text));
                    states
                }
            },
        }
    }
}

struct RegexParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> RegexParser<'a> {
    fn new(pattern: &'a str) -> Self {
        Self {
            chars: pattern.chars().peekable(),
        }
    }

    fn is_eof(&mut self) -> bool {
        self.chars.peek().is_none()
    }

    fn parse_expression(&mut self) -> Option<Expr> {
        let mut alternatives = Vec::new();
        loop {
            alternatives.push(self.parse_sequence()?);
            if self.peek() == Some('|') {
                self.chars.next();
                continue;
            }
            break;
        }

        Some(match alternatives.as_slice() {
            [only] => only.clone(),
            _ => Expr::Alternate(alternatives),
        })
    }

    fn parse_sequence(&mut self) -> Option<Expr> {
        let mut items = Vec::new();
        while let Some(ch) = self.peek() {
            if ch == ')' || ch == '|' {
                break;
            }

            items.push(self.parse_atom()?);
        }

        Some(match items.as_slice() {
            [] => Expr::Empty,
            [only] => only.clone(),
            _ => Expr::Sequence(items),
        })
    }

    fn parse_atom(&mut self) -> Option<Expr> {
        let mut atom = match self.chars.next()? {
            '\\' => Expr::Literal(self.chars.next()?.to_string()),
            '.' => Expr::AnyChar,
            '(' => {
                let expr = self.parse_expression()?;
                if self.chars.next()? != ')' {
                    return None;
                }
                expr
            }
            '[' => {
                let mut literal = String::from("[");
                while let Some(ch) = self.chars.next() {
                    literal.push(ch);
                    if ch == ']' {
                        break;
                    }
                }
                Expr::Literal(literal)
            }
            ch => Expr::Literal(ch.to_string()),
        };

        if let Some(next) = self.peek() {
            atom = match next {
                '*' => {
                    self.chars.next();
                    Expr::Repeat {
                        expr: Box::new(atom),
                        kind: RepeatKind::ZeroOrMore,
                    }
                }
                '?' => {
                    self.chars.next();
                    Expr::Repeat {
                        expr: Box::new(atom),
                        kind: RepeatKind::ZeroOrOne,
                    }
                }
                _ => atom,
            };
        }

        Some(atom)
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }
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
    fn watchtower_only_and_allow_all_work() {
        let watchtower = Container {
            watchtower: true,
            ..container("watchtower")
        };
        let regular = container("regular");

        assert!(watchtower_only(&watchtower));
        assert!(!watchtower_only(&regular));
        assert!(allow_all(&regular));
    }

    #[test]
    fn by_name_matches_exact_and_legacy_regex_like_patterns() {
        let names = vec!["test".to_string(), "ba(b|ll)oon".to_string()];
        let filter = by_name(&names, Box::new(allow_all::<Container>));

        assert!(filter(&container("test")));
        assert!(filter(&container("/test")));
        assert!(filter(&container("balloon")));
        assert!(!filter(&container("spoon")));
        assert!(!filter(&container("baboonious")));
    }

    #[test]
    fn by_disable_name_uses_exact_name_matching_only() {
        let names = vec!["excluded".to_string()];
        let filter = by_disable_name(&names, Box::new(allow_all::<Container>));

        assert!(!filter(&container("excluded")));
        assert!(!filter(&container("/excluded")));
        assert!(filter(&container("excludedAsSubstring")));
    }

    #[test]
    fn enable_and_disabled_label_behave_like_the_legacy_code() {
        let enable_filter = by_enable_label(Box::new(allow_all::<Container>));
        let disabled_filter = by_disabled_label(Box::new(allow_all::<Container>));

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
    fn scope_none_treats_missing_and_empty_scope_as_none() {
        let filter = by_scope("none", Box::new(allow_all::<Container>));

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
    fn by_image_ignores_tags_and_preserves_base_filter() {
        let images = vec!["registry".to_string(), "other".to_string()];
        let filter = by_image(Some(&images), Box::new(allow_all::<Container>));
        let no_images = by_image(None, Box::new(allow_all::<Container>));

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
    fn build_filter_matches_the_legacy_composition_order() {
        let names = vec!["test".to_string()];
        let disable_names = vec!["skip".to_string()];
        let filter = build_filter::<Container>(&names, &disable_names, true, Some("none"));

        let matching = Container {
            enabled: (true, true),
            scope: (None, false),
            ..container("test")
        };
        let wrong_scope = Container {
            enabled: (true, true),
            scope: (Some("team".to_string()), true),
            ..container("test")
        };
        let disabled = Container {
            enabled: (false, true),
            scope: (None, false),
            ..container("test")
        };

        assert!(filter(&matching));
        assert!(!filter(&wrong_scope));
        assert!(!filter(&disabled));
    }
}
