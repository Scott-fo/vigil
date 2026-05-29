use std::path::PathBuf;

use super::super::{App, WorktreeEntry};

fn entry(path: &str, branch: Option<&str>, dirty: bool) -> WorktreeEntry {
    WorktreeEntry {
        path: PathBuf::from(path),
        head: Some("abcdef0123456789".to_string()),
        branch: branch.map(ToOwned::to_owned),
        detached: branch.is_none(),
        bare: false,
        prunable: false,
        dirty,
        change_count: usize::from(dirty),
    }
}

#[test]
fn worktree_filter_matches_branch_path_and_state() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/repo/main"));
    app.worktree_entries = vec![
        entry("/repo/main", Some("main"), false),
        entry("/repo/feature-auth", Some("feature/auth"), true),
    ];

    app.worktree_query = "dirty auth".to_string();

    assert_eq!(app.filtered_worktree_indices(), vec![1]);
}

#[test]
fn seed_worktree_selection_prefers_current_repo_root() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/repo/feature-auth"));
    app.worktree_entries = vec![
        entry("/repo/main", Some("main"), false),
        entry("/repo/feature-auth", Some("feature/auth"), true),
    ];

    app.seed_worktree_selection();

    assert_eq!(app.worktree_selected_index, 1);
}
