use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub(super) fn is_relevant_event(event: &NotifyEvent) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }

    if event.paths.is_empty() {
        return true;
    }

    event
        .paths
        .iter()
        .any(|path| !should_ignore_event_path(path))
}

pub(super) fn maybe_watch_new_directories(
    watcher_ref: &Arc<Mutex<Option<RecommendedWatcher>>>,
    paths: &[PathBuf],
) {
    if paths.is_empty() {
        return;
    }

    let mut guard = match watcher_ref.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    let Some(watcher) = guard.as_mut() else {
        return;
    };

    for path in paths {
        if should_ignore_event_path(path) {
            continue;
        }
        if let Ok(metadata) = std::fs::metadata(path)
            && metadata.is_dir()
        {
            let _ = watcher.watch(path, RecursiveMode::NonRecursive);
        }
    }
}

fn should_ignore_event_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized == ".git" || normalized.ends_with("/.git") || normalized.contains("/.git/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_git_directory_paths() {
        assert!(should_ignore_event_path(Path::new(".git")));
        assert!(should_ignore_event_path(Path::new("repo/.git")));
        assert!(should_ignore_event_path(Path::new("repo/.git/index")));
        assert!(!should_ignore_event_path(Path::new("repo/src/git.rs")));
    }
}
