use std::{collections::HashSet, path::PathBuf, sync::Arc};

use nucleo_matcher::Matcher;
use ratatui::widgets::ListState;
use strum_macros::{EnumString, IntoStaticStr};
use tokio::task;

mod background;
mod blame_modal;
mod branch_compare;
mod branch_merge;
mod clipboard;
mod commit_modal;
mod commit_search;
mod diff;
mod diff_search;
mod discard_modal;
mod editor;
mod file_search;
mod help_modal;
mod input;
mod keyboard;
mod launch;
mod mouse;
mod navigation;
mod repo_state;
mod review;
mod runtime;
mod sidebar_state;
mod theme_modal;
mod working_tree_actions;
mod worktree;

pub use self::diff::{DiffCacheKey, PreparedDiffViewport};
use self::diff::{DiffHighlightJob, DiffViewCache, DiffViewport};
use self::diff_search::DiffSearchNavigationTarget;
pub use self::launch::AppLaunchOptions;
use crate::{
    event::{DiffPrefetchedEvent, Event, EventHandler},
    git::{
        self, BlameCommitDetails, BlameTarget, BranchCompareSelection, CommitCompareSelection,
        CommitSearchEntry, DiffSearchIndex, DiffSearchResults, DiffSelectionPoint, DiffView,
        FileEntry, SharedHighlightRegistry, WorktreeEntry,
    },
    review::ReviewReport,
    sidebar::SidebarItem,
    theme::ThemeMode,
    watcher::RepoWatcher,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivePane {
    Sidebar,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DiffViewMode {
    Unified,
    Split,
}

impl DiffViewMode {
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCompareField {
    Source,
    Destination,
}

#[derive(Debug, Clone)]
pub enum ReviewMode {
    WorkingTree,
    CommitCompare(CommitCompareSelection),
    BranchCompare(BranchCompareSelection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteSyncDirection {
    Pull,
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnackbarVariant {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct SnackbarNotice {
    pub message: String,
    pub variant: SnackbarVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffTextSelection {
    pub anchor: DiffSelectionPoint,
    pub head: DiffSelectionPoint,
}

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub repo_root: PathBuf,
    pub chooser_file_path: Option<PathBuf>,
    pub repo_error: Option<String>,
    pub repo_loading: bool,
    pub events: EventHandler,
    pub active_pane: ActivePane,
    pub review_mode: ReviewMode,
    pub files: Vec<FileEntry>,
    pub sidebar_items: Vec<SidebarItem>,
    pub collapsed_directories: HashSet<String>,
    pub sidebar_state: ListState,
    pub sidebar_scroll: usize,
    pub sidebar_viewport_height: usize,
    pub sidebar_hidden: bool,
    pub selected_sidebar_row: usize,
    pub selected_file_index: usize,
    pub diff_view: DiffView,
    pub diff_view_mode: DiffViewMode,
    pub diff_scroll: u16,
    pub selected_diff_line_index: usize,
    pub diff_text_selection: Option<DiffTextSelection>,
    diff_text_selection_anchor: Option<DiffSelectionPoint>,
    repo_request_id: u64,
    pub diff_request_id: u64,
    diff_load_task: Option<task::JoinHandle<()>>,
    diff_highlight_task: Option<task::JoinHandle<()>>,
    diff_highlight_job: Option<DiffHighlightJob>,
    diff_highlight_complete: bool,
    diff_viewport: Option<DiffViewport>,
    background_tasks: Vec<task::JoinHandle<()>>,
    diff_prefetch_task: Option<task::JoinHandle<()>>,
    diff_view_cache: DiffViewCache,
    diff_cache_generation: u64,
    pending_diff_cache_key: Option<DiffCacheKey>,
    pub highlight_registry: Option<SharedHighlightRegistry>,
    highlight_registry_loading: bool,
    pub repo_watcher: Option<RepoWatcher>,
    pub repo_watcher_loading: bool,
    pub blame_modal_open: bool,
    pub blame_target: Option<BlameTarget>,
    pub blame_loading: bool,
    pub blame_details: Option<BlameCommitDetails>,
    pub blame_error: Option<String>,
    pub blame_scroll: u16,
    pub blame_request_id: u64,
    blame_load_task: Option<task::JoinHandle<()>>,
    pub help_modal_open: bool,
    pub theme_modal_open: bool,
    pub theme_modal_query: String,
    pub theme_modal_selected_index: usize,
    pub theme_modal_initial_name: String,
    pub theme_modal_initial_mode: ThemeMode,
    pub theme_name: String,
    pub theme_mode: ThemeMode,
    pub theme_matcher: Matcher,
    pub file_search_modal_open: bool,
    pub file_search_query: String,
    pub file_search_selected_index: usize,
    pub file_search_initial_path: Option<String>,
    pub file_search_matcher: Matcher,
    find_prefix_pending: bool,
    pub diff_search_modal_open: bool,
    pub diff_search_query: String,
    pub diff_search_loading: bool,
    pub diff_search_error: Option<String>,
    pub diff_search_results: DiffSearchResults,
    pub diff_search_selected_index: usize,
    diff_search_index: Option<Arc<DiffSearchIndex>>,
    diff_search_index_error: Option<String>,
    diff_search_index_request_id: u64,
    diff_search_query_request_id: u64,
    diff_search_load_task: Option<task::JoinHandle<()>>,
    diff_search_query_task: Option<task::JoinHandle<()>>,
    pending_diff_search_target: Option<DiffSearchNavigationTarget>,
    pub commit_search_modal_open: bool,
    pub commit_search_query: String,
    pub commit_search_entries: Vec<CommitSearchEntry>,
    pub commit_search_loading: bool,
    pub commit_search_error: Option<String>,
    pub commit_search_selected_index: usize,
    pub commit_search_matcher: Matcher,
    pub branch_compare_modal_open: bool,
    pub branch_compare_loading: bool,
    pub branch_compare_error: Option<String>,
    pub branch_compare_active_field: BranchCompareField,
    pub branch_compare_available_refs: Vec<String>,
    pub branch_compare_source_query: String,
    pub branch_compare_destination_query: String,
    pub branch_compare_source_ref: Option<String>,
    pub branch_compare_destination_ref: Option<String>,
    pub branch_compare_selected_source_index: usize,
    pub branch_compare_selected_destination_index: usize,
    pub branch_compare_matcher: Matcher,
    pub branch_merge_target: Option<git::BranchMergeRequest>,
    pub branch_merge_loading: bool,
    pub branch_merge_error: Option<String>,
    pub worktree_modal_open: bool,
    pub worktree_loading: bool,
    pub worktree_error: Option<String>,
    pub worktree_query: String,
    pub worktree_entries: Vec<WorktreeEntry>,
    pub worktree_selected_index: usize,
    pub worktree_matcher: Matcher,
    pub commit_modal_open: bool,
    pub commit_message: String,
    pub commit_error: Option<String>,
    pub discard_target: Option<FileEntry>,
    pub remote_sync: Option<RemoteSyncDirection>,
    pub review_loading: bool,
    pub review_error: Option<String>,
    pub review_report: Option<ReviewReport>,
    pub review_snapshot_id: Option<String>,
    pub review_provider_session_id: Option<String>,
    pub review_context_modal_open: bool,
    pub review_extra_context: String,
    pub review_summary_modal_open: bool,
    pub review_summary_scroll: u16,
    review_request_id: u64,
    review_task: Option<task::JoinHandle<()>>,
    pub snackbar_notice: Option<SnackbarNotice>,
    pub snackbar_generation: u64,
    pub status_message: Option<String>,
}

#[cfg(test)]
mod tests;
