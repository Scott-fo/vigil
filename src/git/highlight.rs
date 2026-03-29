use std::{
    cell::RefCell,
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

use color_eyre::eyre::WrapErr;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

const EXACT_HIGHLIGHT_CACHE_CAPACITY: usize = 8;

static HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "attribute.builtin",
    "boolean",
    "character",
    "character.special",
    "comment",
    "comment.documentation",
    "conditional",
    "constant",
    "constant.builtin",
    "constructor",
    "constructor.builtin",
    "delimiter",
    "embedded",
    "exception",
    "field",
    "function",
    "function.builtin",
    "function.call",
    "function.method",
    "function.method.call",
    "function.method.builtin",
    "function.macro",
    "function.special",
    "keyword",
    "keyword.conditional",
    "keyword.conditional.ternary",
    "keyword.coroutine",
    "keyword.debug",
    "keyword.directive",
    "keyword.exception",
    "keyword.function",
    "keyword.import",
    "keyword.modifier",
    "keyword.operator",
    "keyword.repeat",
    "keyword.return",
    "keyword.type",
    "label",
    "method",
    "method.call",
    "markup.heading",
    "markup.heading.1",
    "markup.heading.2",
    "markup.heading.3",
    "markup.heading.4",
    "markup.heading.5",
    "markup.heading.6",
    "markup.link",
    "markup.link.label",
    "markup.link.url",
    "markup.list",
    "markup.list.checked",
    "markup.list.unchecked",
    "markup.quote",
    "markup.raw",
    "markup.raw.block",
    "module",
    "module.builtin",
    "namespace",
    "number",
    "number.float",
    "operator",
    "parameter",
    "property",
    "property.definition",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "repeat",
    "string",
    "string.escape",
    "string.regexp",
    "string.special",
    "string.special.url",
    "string.special.key",
    "string.special.path",
    "string.special.regex",
    "string.special.symbol",
    "string.special.uri",
    "tag",
    "tag.attribute",
    "tag.builtin",
    "tag.delimiter",
    "tag.error",
    "type",
    "type.builtin",
    "type.definition",
    "type.qualifier",
    "variable",
    "variable.builtin",
    "variable.member",
    "variable.parameter",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxToken {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) highlight_name: Option<&'static str>,
}

pub struct HighlightRegistry {
    configs: Mutex<HashMap<&'static str, Arc<QueryHighlightConfig>>>,
}

struct QueryHighlightConfig {
    language: tree_sitter::Language,
    query: Query,
    capture_highlight_names: Box<[Option<&'static str>]>,
}

struct CachedSyntaxRunner {
    parser: Parser,
    query_cursor: QueryCursor,
}

struct ExactHighlightCacheEntry {
    filetype: &'static str,
    source_hash: u64,
    source_len: usize,
    source: Arc<str>,
    highlighted_lines: Arc<[Vec<SyntaxToken>]>,
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

fn build_highlight_config(
    filetype: &'static str,
) -> color_eyre::Result<Option<QueryHighlightConfig>> {
    let mut configs = HashMap::new();
    let ecma_highlights = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/ecma/highlights.scm"
    ));
    let ecma_locals = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/ecma/locals.scm"
    ));
    let ecma_injections = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/ecma/injections.scm"
    ));
    let jsx_nvim_highlights = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/jsx/highlights.scm"
    ));
    let jsx_nvim_injections = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/jsx/injections.scm"
    ));
    let typescript_highlights_query = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/typescript/highlights.scm"
    ));
    let typescript_locals_query = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/typescript/locals.scm"
    ));
    let typescript_injections_query = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/vendor/nvim-treesitter/typescript/injections.scm"
    ));

    match filetype {
        "rust" => register_highlight_config(
            &mut configs,
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/rust/highlights.scm"
            )),
            "",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/rust/locals.scm"
            )),
        )?,
        "javascript" => register_highlight_config(
            &mut configs,
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        )?,
        "jsx" => {
            let jsx_highlights = format!(
                "{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
            );
            register_highlight_config(
                &mut configs,
                "jsx",
                tree_sitter_javascript::LANGUAGE.into(),
                "javascript",
                &jsx_highlights,
                tree_sitter_javascript::INJECTIONS_QUERY,
                tree_sitter_javascript::LOCALS_QUERY,
            )?;
        }
        "typescript" => {
            let typescript_highlights = format!("{ecma_highlights}\n{typescript_highlights_query}");
            let typescript_locals = format!("{ecma_locals}\n{typescript_locals_query}");
            let typescript_injections = format!("{ecma_injections}\n{typescript_injections_query}");
            register_highlight_config(
                &mut configs,
                "typescript",
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                "typescript",
                &typescript_highlights,
                &typescript_injections,
                &typescript_locals,
            )?;
        }
        "tsx" => {
            let typescript_locals = format!("{ecma_locals}\n{typescript_locals_query}");
            let tsx_highlights =
                format!("{ecma_highlights}\n{typescript_highlights_query}\n{jsx_nvim_highlights}");
            let tsx_injections =
                format!("{ecma_injections}\n{typescript_injections_query}\n{jsx_nvim_injections}");
            register_highlight_config(
                &mut configs,
                "tsx",
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                "tsx",
                &tsx_highlights,
                &tsx_injections,
                &typescript_locals,
            )?;
        }
        "python" => register_highlight_config(
            &mut configs,
            "python",
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "go" => register_highlight_config(
            &mut configs,
            "go",
            tree_sitter_go::LANGUAGE.into(),
            "go",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/go/highlights.scm"
            )),
            "",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/nvim-treesitter/go/locals.scm"
            )),
        )?,
        "c" => register_highlight_config(
            &mut configs,
            "c",
            tree_sitter_c::LANGUAGE.into(),
            "c",
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        )?,
        "cpp" => register_highlight_config(
            &mut configs,
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            "cpp",
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        )?,
        "csharp" => register_highlight_config(
            &mut configs,
            "csharp",
            tree_sitter_c_sharp::LANGUAGE.into(),
            "c_sharp",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/vendor/tree-sitter-c-sharp/highlights.scm"
            )),
            "",
            "",
        )?,
        "bash" => register_highlight_config(
            &mut configs,
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        )?,
        "java" => register_highlight_config(
            &mut configs,
            "java",
            tree_sitter_java::LANGUAGE.into(),
            "java",
            tree_sitter_java::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "ruby" => register_highlight_config(
            &mut configs,
            "ruby",
            tree_sitter_ruby::LANGUAGE.into(),
            "ruby",
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ruby::LOCALS_QUERY,
        )?,
        "php" => register_highlight_config(
            &mut configs,
            "php",
            tree_sitter_php::LANGUAGE_PHP.into(),
            "php",
            tree_sitter_php::HIGHLIGHTS_QUERY,
            tree_sitter_php::INJECTIONS_QUERY,
            "",
        )?,
        "scala" => register_highlight_config(
            &mut configs,
            "scala",
            tree_sitter_scala::LANGUAGE.into(),
            "scala",
            tree_sitter_scala::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_scala::LOCALS_QUERY,
        )?,
        "html" => register_highlight_config(
            &mut configs,
            "html",
            tree_sitter_html::LANGUAGE.into(),
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        )?,
        "json" => register_highlight_config(
            &mut configs,
            "json",
            tree_sitter_json::LANGUAGE.into(),
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "yaml" => register_highlight_config(
            &mut configs,
            "yaml",
            tree_sitter_yaml::LANGUAGE.into(),
            "yaml",
            tree_sitter_yaml::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "haskell" => register_highlight_config(
            &mut configs,
            "haskell",
            tree_sitter_haskell::LANGUAGE.into(),
            "haskell",
            tree_sitter_haskell::HIGHLIGHTS_QUERY,
            tree_sitter_haskell::INJECTIONS_QUERY,
            tree_sitter_haskell::LOCALS_QUERY,
        )?,
        "css" => register_highlight_config(
            &mut configs,
            "css",
            tree_sitter_css::LANGUAGE.into(),
            "css",
            tree_sitter_css::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        "nix" => register_highlight_config(
            &mut configs,
            "nix",
            tree_sitter_nix::LANGUAGE.into(),
            "nix",
            tree_sitter_nix::HIGHLIGHTS_QUERY,
            tree_sitter_nix::INJECTIONS_QUERY,
            "",
        )?,
        "zig" => register_highlight_config(
            &mut configs,
            "zig",
            tree_sitter_zig::LANGUAGE.into(),
            "zig",
            tree_sitter_zig::HIGHLIGHTS_QUERY,
            "",
            "",
        )?,
        _ => return Ok(None),
    }

    Ok(configs.remove(filetype))
}

thread_local! {
    static SYNTAX_RUNNERS: RefCell<HashMap<&'static str, CachedSyntaxRunner>> =
        RefCell::new(HashMap::new());
    static EXACT_HIGHLIGHT_CACHE: RefCell<Vec<ExactHighlightCacheEntry>> =
        const { RefCell::new(Vec::new()) };
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

    let source_hash = hash_source(source.as_ref());
    let source_len = source.len();

    if let Some(hit) = EXACT_HIGHLIGHT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let position = cache.iter().position(|entry| {
            entry.filetype == filetype
                && entry.source_hash == source_hash
                && entry.source_len == source_len
                && entry.source.as_ref() == source.as_ref()
        })?;
        let entry = cache.remove(position);
        let highlighted_lines = entry.highlighted_lines.clone();
        cache.push(entry);
        Some(highlighted_lines)
    }) {
        return Some(hit);
    }

    let highlighted_lines = Arc::<[Vec<SyntaxToken>]>::from(
        highlight_source_lines(registry, filetype, source.as_ref())?.into_boxed_slice(),
    );
    EXACT_HIGHLIGHT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.push(ExactHighlightCacheEntry {
            filetype,
            source_hash,
            source_len,
            source: source.clone(),
            highlighted_lines: highlighted_lines.clone(),
        });
        if cache.len() > EXACT_HIGHLIGHT_CACHE_CAPACITY {
            let overflow = cache.len() - EXACT_HIGHLIGHT_CACHE_CAPACITY;
            cache.drain(..overflow);
        }
    });
    Some(highlighted_lines)
}

fn hash_source(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

pub fn clear_exact_highlight_cache() {
    EXACT_HIGHLIGHT_CACHE.with(|cache| cache.borrow_mut().clear());
}

pub fn prewarm_highlight_registry<I>(
    registry: &HighlightRegistry,
    filetypes: I,
) -> color_eyre::Result<()>
where
    I: IntoIterator<Item = &'static str>,
{
    for filetype in filetypes {
        let _ = registry.ensure_filetype(filetype)?;
        if let Some(sample) = sample_source_for_filetype(filetype) {
            let _ = highlight_source_lines(registry, filetype, sample);
        }
    }
    Ok(())
}

fn sample_source_for_filetype(filetype: &'static str) -> Option<&'static str> {
    match filetype {
        "rust" => Some("fn build_user(id: usize) -> User { User::new(id) }"),
        "go" => Some("func BuildUser(id int) User { return NewUser(id) }"),
        "typescript" => Some("const user: User = await loadUser(id);"),
        "tsx" => Some("<Card title=\"demo\">{value}</Card>"),
        "javascript" => Some("const user = await loadUser(id);"),
        "jsx" => Some("<Card>{value}</Card>"),
        "python" => Some("def build_user(id: int) -> User:\n    return User(id)"),
        "bash" => Some("build_user() { echo \"$1\"; }"),
        "java" => Some("class User { String name() { return value; } }"),
        "ruby" => Some("def build_user(id) = User.new(id)"),
        "php" => Some("<?php function buildUser($id) { return new User($id); }"),
        "scala" => Some("def buildUser(id: Int): User = User(id)"),
        "html" => Some("<div class=\"card\">demo</div>"),
        "json" => Some("{\"user\": {\"id\": 1}}"),
        "yaml" => Some("user:\n  id: 1"),
        "css" => Some(".card { color: red; }"),
        "c" => Some("int build_user(int id) { return id; }"),
        "cpp" => Some("int build_user(int id) { return id; }"),
        "csharp" => Some("class User { string Name() => value; }"),
        "haskell" => Some("buildUser id = User id"),
        "nix" => Some("{ user = { id = 1; }; }"),
        "zig" => Some(
            "const User = struct { id: usize }; fn buildUser(id: usize) User { return .{ .id = id }; }",
        ),
        "markdown" => Some("# Prefetch"),
        _ => None,
    }
}

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

#[derive(Clone, Copy)]
struct QueryHighlightRange {
    start: usize,
    end: usize,
    highlight_name: Option<&'static str>,
    specificity: u8,
}

fn query_captures_to_lines(
    query_cursor: &mut QueryCursor,
    query: &Query,
    capture_highlight_names: &[Option<&'static str>],
    root_node: tree_sitter::Node<'_>,
    source: &str,
) -> Option<Vec<Vec<SyntaxToken>>> {
    let mut ranges = Vec::new();
    let mut captures = query_cursor.captures(query, root_node, source.as_bytes());
    while {
        captures.advance();
        captures.get().is_some()
    } {
        let Some((query_match, capture_index)) = captures.get() else {
            continue;
        };
        let Some(query_capture) = query_match.captures.get(*capture_index) else {
            continue;
        };
        let start = query_capture.node.start_byte();
        let end = query_capture.node.end_byte();
        if start >= end || end > source.len() {
            continue;
        }
        let highlight_name = capture_highlight_names
            .get(query_capture.index as usize)
            .copied()
            .flatten();
        let specificity = highlight_name
            .map(|name| name.split('.').count() as u8)
            .unwrap_or(0);
        ranges.push(QueryHighlightRange {
            start,
            end,
            highlight_name,
            specificity,
        });
    }

    if ranges.is_empty() {
        return Some(
            source
                .split('\n')
                .map(|line| {
                    vec![SyntaxToken {
                        start: 0,
                        end: line.len(),
                        highlight_name: None,
                    }]
                })
                .collect(),
        );
    }

    let mut lines = Vec::new();
    let mut current_line = Vec::new();
    let mut active_ranges = Vec::new();
    let mut active_endings = BinaryHeap::new();
    let mut current_offset = 0usize;
    let mut current_line_start = 0usize;
    let mut next_range_index = 0usize;

    while next_range_index < ranges.len() || !active_endings.is_empty() {
        let next_start = ranges
            .get(next_range_index)
            .map(|range| range.start)
            .unwrap_or(usize::MAX);
        let next_end = active_endings
            .peek()
            .map(|ending: &Reverse<(usize, usize)>| ending.0.0)
            .unwrap_or(usize::MAX);
        let next_offset = next_start.min(next_end);

        if current_offset < next_offset {
            let highlight_name = select_active_highlight_name(&active_ranges, &ranges);
            push_highlighted_source_segment(
                &mut lines,
                &mut current_line,
                source,
                current_offset,
                next_offset,
                &mut current_line_start,
                highlight_name,
            );
        }

        if next_end <= next_start {
            while let Some(Reverse((end, range_index))) = active_endings.peek().copied() {
                if end != next_end {
                    break;
                }
                let _ = active_endings.pop();
                if let Some(position) = active_ranges
                    .iter()
                    .position(|active_range_index| *active_range_index == range_index)
                {
                    active_ranges.swap_remove(position);
                }
            }
            current_offset = next_end;
            continue;
        }

        while let Some(range) = ranges.get(next_range_index) {
            if range.start != next_start {
                break;
            }
            active_ranges.push(next_range_index);
            active_endings.push(Reverse((range.end, next_range_index)));
            next_range_index += 1;
        }
        current_offset = next_start;
    }

    if current_offset < source.len() {
        let highlight_name = select_active_highlight_name(&active_ranges, &ranges);
        push_highlighted_source_segment(
            &mut lines,
            &mut current_line,
            source,
            current_offset,
            source.len(),
            &mut current_line_start,
            highlight_name,
        );
    }

    lines.push(current_line);
    Some(lines)
}

fn select_active_highlight_name(
    active_ranges: &[usize],
    ranges: &[QueryHighlightRange],
) -> Option<&'static str> {
    active_ranges
        .iter()
        .copied()
        .max_by_key(|range_index| {
            let range = ranges[*range_index];
            (range.specificity, *range_index)
        })
        .and_then(|range_index| ranges[range_index].highlight_name)
}

fn push_highlighted_source_segment(
    lines: &mut Vec<Vec<SyntaxToken>>,
    current_line: &mut Vec<SyntaxToken>,
    source: &str,
    mut start: usize,
    end: usize,
    current_line_start: &mut usize,
    highlight_name: Option<&'static str>,
) {
    while start < end {
        let segment = &source[start..end];
        if let Some(newline_offset) = segment.find('\n') {
            let line_end = start + newline_offset;
            push_syntax_token(
                current_line,
                start.saturating_sub(*current_line_start),
                line_end.saturating_sub(*current_line_start),
                highlight_name,
            );
            lines.push(std::mem::take(current_line));
            start = line_end + 1;
            *current_line_start = start;
        } else {
            push_syntax_token(
                current_line,
                start.saturating_sub(*current_line_start),
                end.saturating_sub(*current_line_start),
                highlight_name,
            );
            break;
        }
    }
}

fn highlight_markdown_inline_tokens(text: &str) -> Vec<SyntaxToken> {
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < text.len() {
        let remainder = &text[index..];

        if let Some(rest) = remainder.strip_prefix('`')
            && let Some(end) = rest.find('`')
        {
            let code_end = index + 1 + end + 1;
            push_syntax_token(&mut tokens, index, code_end, Some("markup.raw"));
            index = code_end;
            continue;
        }

        if let Some(label_end) = remainder.find("](")
            && remainder.starts_with('[')
            && let Some(url_end) = remainder[label_end + 2..].find(')')
        {
            let label_text_end = index + label_end + 1;
            let url_start = index + label_end + 2;
            let url_end = url_start + url_end;
            push_syntax_token(&mut tokens, index, index + 1, None);
            push_syntax_token(
                &mut tokens,
                index + 1,
                label_text_end,
                Some("markup.link.label"),
            );
            push_syntax_token(&mut tokens, label_text_end, label_text_end + 2, None);
            push_syntax_token(&mut tokens, url_start, url_end, Some("markup.link.url"));
            push_syntax_token(&mut tokens, url_end, url_end + 1, None);
            index = url_end + 1;
            continue;
        }

        let mut next_break = remainder.len();
        for needle in ["`", "["] {
            if let Some(found) = remainder.find(needle) {
                next_break = next_break.min(found);
            }
        }
        if next_break == 0 {
            next_break = remainder.chars().next().map(char::len_utf8).unwrap_or(1);
        }
        push_syntax_token(&mut tokens, index, index + next_break, None);
        index += next_break;
    }

    tokens
}

fn markdown_list_prefix_len(text: &str) -> Option<usize> {
    for marker in ["- ", "* ", "+ "] {
        if text.starts_with(marker) {
            return Some(marker.len());
        }
    }

    let digit_count = text.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count > 0 {
        let remainder = &text[digit_count..];
        if remainder.starts_with(". ") || remainder.starts_with(") ") {
            return Some(digit_count + 2);
        }
    }

    None
}

fn highlight_markdown_line_tokens(line: &str) -> Vec<SyntaxToken> {
    let mut tokens = Vec::new();
    let indent_len = line.len() - line.trim_start().len();
    let (_, trimmed) = line.split_at(indent_len);
    push_syntax_token(&mut tokens, 0, indent_len, None);

    if trimmed.is_empty() {
        return tokens;
    }

    let bare = trimmed.trim();
    if bare.len() >= 3 && bare.chars().all(|ch| matches!(ch, '-' | '*' | '_')) {
        push_syntax_token(&mut tokens, indent_len, line.len(), Some("operator"));
        return tokens;
    }

    for fence in ["```", "~~~"] {
        if let Some(rest) = trimmed.strip_prefix(fence) {
            push_syntax_token(
                &mut tokens,
                indent_len,
                indent_len + fence.len(),
                Some("markup.raw"),
            );
            let ws_len = rest.len() - rest.trim_start().len();
            let info_start = indent_len + fence.len() + ws_len;
            push_syntax_token(&mut tokens, indent_len + fence.len(), info_start, None);
            push_syntax_token(&mut tokens, info_start, line.len(), Some("label"));
            return tokens;
        }
    }

    if let Some(rest) = trimmed.strip_prefix("> ") {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + 2,
            Some("markup.quote"),
        );
        tokens.extend(
            highlight_markdown_inline_tokens(rest)
                .into_iter()
                .map(|token| SyntaxToken {
                    start: token.start + indent_len + 2,
                    end: token.end + indent_len + 2,
                    highlight_name: token.highlight_name,
                }),
        );
        return tokens;
    }

    let heading_marker_len = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&heading_marker_len) && trimmed[heading_marker_len..].starts_with(' ') {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + heading_marker_len,
            Some("markup.heading"),
        );
        push_syntax_token(
            &mut tokens,
            indent_len + heading_marker_len,
            indent_len + heading_marker_len + 1,
            None,
        );
        push_syntax_token(
            &mut tokens,
            indent_len + heading_marker_len + 1,
            line.len(),
            Some("markup.heading"),
        );
        return tokens;
    }

    if let Some(prefix_len) = markdown_list_prefix_len(trimmed) {
        push_syntax_token(
            &mut tokens,
            indent_len,
            indent_len + prefix_len,
            Some("markup.list"),
        );
        let rest = &trimmed[prefix_len..];
        let rest_start = indent_len + prefix_len;
        if let Some(task_rest) = rest.strip_prefix("[ ] ") {
            push_syntax_token(
                &mut tokens,
                rest_start,
                rest_start + 4,
                Some("markup.list.unchecked"),
            );
            tokens.extend(
                highlight_markdown_inline_tokens(task_rest)
                    .into_iter()
                    .map(|token| SyntaxToken {
                        start: token.start + rest_start + 4,
                        end: token.end + rest_start + 4,
                        highlight_name: token.highlight_name,
                    }),
            );
            return tokens;
        }
        if let Some(task_rest) = rest
            .strip_prefix("[x] ")
            .or_else(|| rest.strip_prefix("[X] "))
        {
            push_syntax_token(
                &mut tokens,
                rest_start,
                rest_start + 4,
                Some("markup.list.checked"),
            );
            tokens.extend(
                highlight_markdown_inline_tokens(task_rest)
                    .into_iter()
                    .map(|token| SyntaxToken {
                        start: token.start + rest_start + 4,
                        end: token.end + rest_start + 4,
                        highlight_name: token.highlight_name,
                    }),
            );
            return tokens;
        }
        tokens.extend(
            highlight_markdown_inline_tokens(rest)
                .into_iter()
                .map(|token| SyntaxToken {
                    start: token.start + rest_start,
                    end: token.end + rest_start,
                    highlight_name: token.highlight_name,
                }),
        );
        return tokens;
    }

    tokens.extend(
        highlight_markdown_inline_tokens(trimmed)
            .into_iter()
            .map(|token| SyntaxToken {
                start: token.start + indent_len,
                end: token.end + indent_len,
                highlight_name: token.highlight_name,
            }),
    );
    tokens
}

fn register_highlight_config(
    configs: &mut HashMap<&'static str, QueryHighlightConfig>,
    key: &'static str,
    language: tree_sitter::Language,
    _language_name: &'static str,
    highlights: &str,
    _injections: &str,
    _locals: &str,
) -> color_eyre::Result<()> {
    let query = Query::new(&language, highlights)
        .wrap_err_with(|| format!("failed to build {key} query config"))?;
    let capture_highlight_names = query
        .capture_names()
        .iter()
        .map(|name| resolve_highlight_name(name))
        .collect();
    configs.insert(
        key,
        QueryHighlightConfig {
            language,
            query,
            capture_highlight_names,
        },
    );
    Ok(())
}

fn resolve_highlight_name(name: &str) -> Option<&'static str> {
    HIGHLIGHT_NAMES
        .iter()
        .copied()
        .find(|candidate| *candidate == name)
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
}
