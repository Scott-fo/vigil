//! Recognition of machine-generated files.
//!
//! Lockfiles, minified bundles, and test snapshots change in large, unreadable
//! blocks that are rarely reviewed line by line. Callers use
//! [`is_generated_file`] to collapse them by default; the classification is
//! purely path based, so it is cheap and needs no file contents.

/// Exact file names, matched against the last path component.
const GENERATED_FILE_NAMES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lock",
    "bun.lockb",
    "go.sum",
    "poetry.lock",
    "uv.lock",
    "Pipfile.lock",
    "Gemfile.lock",
    "composer.lock",
    "flake.lock",
];

/// File name suffixes, matched against the last path component.
const GENERATED_FILE_SUFFIXES: &[&str] = &[".min.js", ".min.css", ".snap"];

pub fn is_generated_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    GENERATED_FILE_NAMES.contains(&name)
        || GENERATED_FILE_SUFFIXES
            .iter()
            .any(|suffix| name.len() > suffix.len() && name.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::is_generated_file;

    #[test]
    fn lockfiles_are_generated_at_any_depth() {
        for path in [
            "Cargo.lock",
            "web/package-lock.json",
            "yarn.lock",
            "apps/site/pnpm-lock.yaml",
            "bun.lockb",
            "go.sum",
            "poetry.lock",
            "Gemfile.lock",
            "composer.lock",
        ] {
            assert!(is_generated_file(path), "{path}");
        }
    }

    #[test]
    fn minified_bundles_and_snapshots_are_generated() {
        assert!(is_generated_file("dist/app.min.js"));
        assert!(is_generated_file("styles/site.min.css"));
        assert!(is_generated_file("src/__snapshots__/button.test.tsx.snap"));
    }

    #[test]
    fn source_files_with_similar_names_are_not_generated() {
        for path in [
            "src/lock.rs",
            "Cargo.toml",
            "package.json",
            "src/app.js",
            "docs/yarn.lock.md",
            ".snap",
            "src/snapshot.rs",
        ] {
            assert!(!is_generated_file(path), "{path}");
        }
    }
}
