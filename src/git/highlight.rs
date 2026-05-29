use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex},
};

use tree_sitter::{Parser, QueryCursor};

use self::{
    config::{QueryHighlightConfig, build_highlight_config},
    exact_cache::{clear_exact_cache, exact_cache_get, exact_cache_insert},
    markdown::highlight_markdown_line_tokens,
    ranges::query_captures_to_lines,
};

mod config;
mod exact_cache;
mod markdown;
mod prewarm;
mod ranges;

pub use self::prewarm::prewarm_highlight_registry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxToken {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) highlight_name: Option<&'static str>,
}

pub struct HighlightRegistry {
    configs: Mutex<HashMap<&'static str, Arc<QueryHighlightConfig>>>,
}

struct CachedSyntaxRunner {
    parser: Parser,
    query_cursor: QueryCursor,
}

impl std::fmt::Debug for HighlightRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let config_count = self
            .configs
            .lock()
            .expect("highlight registry mutex poisoned")
            .len();
        f.debug_struct("HighlightRegistry")
            .field("config_count", &config_count)
            .finish()
    }
}

impl HighlightRegistry {
    pub fn new() -> color_eyre::Result<Self> {
        Self::new_for_filetypes(Self::all_filetypes().iter().copied())
    }

    pub fn new_for_filetypes<I>(filetypes: I) -> color_eyre::Result<Self>
    where
        I: IntoIterator<Item = &'static str>,
    {
        let registry = Self {
            configs: Mutex::new(HashMap::new()),
        };
        registry.ensure_filetypes(filetypes)?;
        Ok(registry)
    }

    pub fn all_filetypes() -> &'static [&'static str] {
        &[
            "rust",
            "javascript",
            "jsx",
            "typescript",
            "tsx",
            "python",
            "go",
            "c",
            "cpp",
            "csharp",
            "bash",
            "java",
            "ruby",
            "php",
            "scala",
            "html",
            "json",
            "yaml",
            "haskell",
            "css",
            "nix",
            "zig",
        ]
    }

    pub fn ensure_filetypes<I>(&self, filetypes: I) -> color_eyre::Result<()>
    where
        I: IntoIterator<Item = &'static str>,
    {
        for filetype in filetypes {
            let _ = self.ensure_filetype(filetype)?;
        }
        Ok(())
    }

    pub fn ensure_filetype(&self, filetype: &'static str) -> color_eyre::Result<bool> {
        if filetype == "markdown" {
            return Ok(false);
        }

        {
            let configs = self
                .configs
                .lock()
                .expect("highlight registry mutex poisoned");
            if configs.contains_key(filetype) {
                return Ok(false);
            }
        }

        let Some(config) = build_highlight_config(filetype)? else {
            return Ok(false);
        };
        let mut configs = self
            .configs
            .lock()
            .expect("highlight registry mutex poisoned");
        if configs.contains_key(filetype) {
            return Ok(false);
        }
        configs.insert(filetype, Arc::new(config));
        Ok(true)
    }

    fn config(&self, filetype: &'static str) -> Option<Arc<QueryHighlightConfig>> {
        let _ = self.ensure_filetype(filetype);
        let configs = self
            .configs
            .lock()
            .expect("highlight registry mutex poisoned");
        configs.get(filetype).cloned()
    }
}

thread_local! {
    static SYNTAX_RUNNERS: RefCell<HashMap<&'static str, CachedSyntaxRunner>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn highlight_source_lines(
    registry: &HighlightRegistry,
    filetype: &'static str,
    source: &str,
) -> Option<Vec<Vec<SyntaxToken>>> {
    if source.is_empty() {
        return Some(vec![Vec::new()]);
    }

    if filetype == "markdown" {
        return Some(
            source
                .split('\n')
                .map(highlight_markdown_line_tokens)
                .collect(),
        );
    }

    let config = registry.config(filetype)?;
    SYNTAX_RUNNERS.with(|runners| {
        let mut runners = runners.borrow_mut();
        let runner = match runners.entry(filetype) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let mut parser = Parser::new();
                parser.set_language(&config.language).ok()?;
                entry.insert(CachedSyntaxRunner {
                    parser,
                    query_cursor: QueryCursor::new(),
                })
            }
        };
        let tree = runner.parser.parse(source, None)?;
        query_captures_to_lines(
            &mut runner.query_cursor,
            &config.query,
            &config.capture_highlight_names,
            tree.root_node(),
            source,
        )
    })
}

pub(crate) fn highlight_source_lines_cached_exact(
    registry: &HighlightRegistry,
    filetype: &'static str,
    source: &Arc<str>,
) -> Option<Arc<[Vec<SyntaxToken>]>> {
    if source.is_empty() {
        return Some(Arc::from([Vec::new()]));
    }

    if let Some(hit) = exact_cache_get(filetype, source) {
        return Some(hit);
    }

    let highlighted_lines = Arc::<[Vec<SyntaxToken>]>::from(
        highlight_source_lines(registry, filetype, source.as_ref())?.into_boxed_slice(),
    );
    exact_cache_insert(filetype, source, &highlighted_lines);
    Some(highlighted_lines)
}

pub fn clear_exact_highlight_cache() {
    clear_exact_cache();
}

#[inline]
fn push_syntax_token(
    tokens: &mut Vec<SyntaxToken>,
    start: usize,
    end: usize,
    highlight_name: Option<&'static str>,
) {
    if start >= end {
        return;
    }

    if let Some(last) = tokens.last_mut()
        && last.highlight_name == highlight_name
        && last.end == start
    {
        last.end = end;
        return;
    }

    tokens.push(SyntaxToken {
        start,
        end,
        highlight_name,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_highlight_registry() {
        HighlightRegistry::new().expect("highlight registry should initialize");
    }

    #[test]
    fn highlights_rust_go_typescript_zig_and_markdown_without_falling_back() {
        let registry = HighlightRegistry::new().expect("highlight registry should initialize");

        for (filetype, line) in [
            ("rust", "let value = Foo::new(bar);"),
            ("go", "func buildUser(id int) Foo { return NewUser(id) }"),
            ("typescript", "const value: Foo = await loadUser(id);"),
            ("tsx", "<Card title=\"demo\">{value}</Card>"),
            ("zig", "const value = Foo.init(bar);"),
            ("markdown", "# Heading"),
        ] {
            let spans = highlight_source_lines(&registry, filetype, line)
                .expect("highlighting should succeed")
                .pop()
                .unwrap_or_default();
            assert!(
                spans.len() > 1,
                "expected syntax highlighting for {filetype}, got fallback spans: {spans:?}"
            );
        }
    }

    #[test]
    fn tsx_highlighting_keeps_javascript_typescript_and_jsx_captures() {
        let registry =
            HighlightRegistry::new_for_filetypes(["tsx"]).expect("tsx registry should initialize");
        let names = highlight_source_lines(
            &registry,
            "tsx",
            "type Card = { id: string };\nconst view = <section data-id={card.id}>{card.id}</section>;",
        )
        .expect("tsx highlighting should succeed")
        .into_iter()
        .flatten()
        .filter_map(|token| token.highlight_name)
        .collect::<Vec<_>>();

        for expected in [
            "keyword",
            "type",
            "type.builtin",
            "tag.builtin",
            "tag.attribute",
        ] {
            assert!(
                names.contains(&expected),
                "expected {expected} capture in tsx highlight names: {names:?}"
            );
        }
    }
}
