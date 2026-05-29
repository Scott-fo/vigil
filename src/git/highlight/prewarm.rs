use super::{HighlightRegistry, highlight_source_lines};

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

#[cfg(test)]
mod tests {
    use super::{HighlightRegistry, prewarm_highlight_registry, sample_source_for_filetype};

    #[test]
    fn sample_sources_cover_registered_filetypes() {
        for filetype in HighlightRegistry::all_filetypes() {
            assert!(
                sample_source_for_filetype(filetype).is_some(),
                "missing prewarm sample for {filetype}"
            );
        }
    }

    #[test]
    fn prewarm_accepts_markdown_without_registry_config() {
        let registry =
            HighlightRegistry::new_for_filetypes([]).expect("empty registry should initialize");
        prewarm_highlight_registry(&registry, ["markdown"]).expect("markdown should prewarm");
    }
}
