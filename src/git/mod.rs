use std::sync::Arc;

mod diff;
mod highlight;
mod parse;
mod repo;
mod types;

pub use diff::{
    DiffPreviewData, DiffView, GapExpandDirection, build_diff_view_from_diff_text,
    build_diff_view_from_diff_text_with_context, build_diff_view_from_preview_data,
    load_diff_preview_for_branch_compare, load_diff_preview_for_commit_compare,
    load_diff_preview_for_working_tree, load_diff_view, load_diff_view_for_branch_compare,
    load_diff_view_for_commit_compare, load_diff_view_for_working_tree,
};
pub use highlight::{HighlightRegistry, clear_exact_highlight_cache, prewarm_highlight_registry};
pub use repo::{
    commit_staged_changes, discard_file_changes, git_output, init_repo, is_file_staged,
    list_comparable_refs, list_searchable_commits, load_blame_commit_details,
    load_files_with_branch_diff, load_files_with_commit_diff, load_files_with_status,
    load_status_for_path, pull_from_remote, push_to_remote, resolve_commit_base_ref,
    resolve_repo_root, resolve_repo_root_from, should_refresh_for_paths, status_color,
    toggle_file_stage,
};
pub use types::{
    BlameCommitDetails, BlameTarget, BranchCompareSelection, CommitCompareSelection,
    CommitSearchEntry, FileEntry,
};

pub type SharedHighlightRegistry = Arc<HighlightRegistry>;
pub const EMPTY_TREE_HASH: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
pub(crate) const LOG_FIELD_SEPARATOR: char = '\u{001f}';
pub(crate) const LOG_RECORD_SEPARATOR: char = '\u{001e}';
