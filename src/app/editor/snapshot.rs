use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
};

use color_eyre::eyre::{Result, eyre};

pub(super) fn revision_snapshot_path(
    repo_root: &Path,
    revision: &str,
    file_path: &str,
) -> Result<PathBuf> {
    let relative_path = safe_snapshot_relative_path(file_path)?;
    let revision_component = snapshot_component(revision);
    Ok(std::env::temp_dir()
        .join("vigil")
        .join("revision-snapshots")
        .join(format!(
            "{:016x}",
            stable_hash(&repo_root.to_string_lossy())
        ))
        .join(revision_component)
        .join(relative_path))
}

fn safe_snapshot_relative_path(file_path: &str) -> Result<PathBuf> {
    let mut relative_path = PathBuf::new();
    for component in Path::new(file_path).components() {
        match component {
            Component::Normal(part) => relative_path.push(part),
            Component::CurDir => {}
            _ => return Err(eyre!("invalid repository path: {file_path}")),
        }
    }

    if relative_path.as_os_str().is_empty() {
        return Err(eyre!("invalid repository path: {file_path}"));
    }

    Ok(relative_path)
}

fn snapshot_component(value: &str) -> String {
    let mut component = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    component.truncate(80);

    if component.is_empty() {
        component.push_str("revision");
    }

    format!("{component}-{:016x}", stable_hash(value))
}

fn stable_hash<T: Hash + ?Sized>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_snapshot_path_rejects_paths_outside_repo() {
        let root = Path::new("/tmp/repo");

        assert!(revision_snapshot_path(root, "HEAD", "../secret.txt").is_err());
        assert!(revision_snapshot_path(root, "HEAD", "/secret.txt").is_err());
        assert!(revision_snapshot_path(root, "HEAD", "").is_err());
    }

    #[test]
    fn revision_snapshot_path_sanitizes_revision_component() {
        let path = revision_snapshot_path(Path::new("/tmp/repo"), "feature/name~1", "src/lib.rs")
            .expect("snapshot path should resolve");

        let rendered = path.to_string_lossy();
        assert!(rendered.contains("feature_name_1-"));
        assert!(rendered.ends_with("src/lib.rs"));
    }
}
