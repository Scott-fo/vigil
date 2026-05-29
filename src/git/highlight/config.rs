use std::collections::HashMap;

use color_eyre::eyre::WrapErr;
use tree_sitter::Query;

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

pub(super) struct QueryHighlightConfig {
    pub(super) language: tree_sitter::Language,
    pub(super) query: Query,
    pub(super) capture_highlight_names: Box<[Option<&'static str>]>,
}

pub(super) fn build_highlight_config(
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
