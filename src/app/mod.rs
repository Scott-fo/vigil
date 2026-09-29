use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use nucleo_matcher::Matcher;
use ratatui::{layout::Position, widgets::ListState};
use strum_macros::{EnumString, IntoStaticStr};
use tokio::task;

mod background;
mod blame_modal;
mod branch_compare;
mod branch_merge;
mod branches;
mod clipboard;
mod commit_modal;
mod commit_search;
mod diff;
mod diff_search;
mod discard_modal;
mod editor;
mod file_filter;
mod file_search;
mod help_modal;
mod input;
mod keyboard;
mod launch;
mod modal_lookup;
mod mouse;
mod navigation;
mod pull_request;
mod repo_state;
mod runtime;
mod sidebar_state;
mod text_area;
mod theme_modal;
mod viewed;
mod working_tree_actions;
mod worktree;

use self::branches::BranchStatus;
pub use self::branches::{
    BranchPanel, BranchPanelMode, BranchPanelRow, BranchPanelView, BranchSection,
};
pub use self::diff::{DiffCacheKey, DiffStatsState, PreparedDiffViewport};
use self::diff::{
    DiffHighlightJob, DiffPrefetchDirection, DiffViewCache, DiffViewport, PendingChangeLanding,
};
use self::diff_search::{DiffSearchIndexReadiness, DiffSearchNavigationTarget};
use self::file_filter::ExcludeSuffixes;
pub use self::launch::AppLaunchOptions;
use self::modal_lookup::ModalLookupIndex;
use self::pull_request::PullRequests;
#[cfg(test)]
pub(crate) use self::pull_request::fixtures as pull_request_fixtures;
pub use self::pull_request::{
    ActionsMenu, AutoMergeChoice, Composer, ComposerStatus, ComposerTarget, DraftEntry, DraftList,
    MergeBlocker, MergeForm, MergeStep, MergeWhen, MutationOutcome, PULL_REQUEST_LIST_FILTERS,
    PullRequestAction, PullRequestEvent, PullRequestListStatus, PullRequestListView,
    PullRequestModalView, PullRequestOverview, PullRequestPage, PullRequestSelection,
    PullRequestTimer, QueryInput, REVIEW_EVENTS, SubmitForm, SubmitWarning, event_allowed,
};
pub use self::text_area::TextArea;
use crate::{
    event::{DiffPrefetchedEvent, Event, EventHandler},
    git::{
        self, BlameCommitDetails, BlameTarget, BranchCompareSelection, CommitCompareSelection,
        CommitSearchEntry, DiffSearchIndex, DiffSearchMode, DiffSearchResults, DiffSelectionPoint,
        DiffView, FileEntry, ReviewDiffPartialTextIndex, ReviewDiffSnapshot, ReviewDiffTextIndex,
        SharedHighlightRegistry, WorktreeEntry,
    },
    review::{ViewedFiles, ViewedScope},
    sidebar::{DirectoryKey, SidebarItem, SidebarSection},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DiffLineWrapMode {
    Wrap,
    NoWrap,
}

impl DiffLineWrapMode {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Wrap => Self::NoWrap,
            Self::NoWrap => Self::Wrap,
        }
    }

    pub fn is_wrapped(self) -> bool {
        matches!(self, Self::Wrap)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wrap => "wrap",
            Self::NoWrap => "nowrap",
        }
    }
}

impl Default for DiffLineWrapMode {
    fn default() -> Self {
        Self::Wrap
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
    /// A GitHub pull request. Diffs run through the branch-compare path with
    /// the selection's `compare` endpoints.
    PullRequest(PullRequestSelection),
}

/// Which full-screen view fills the terminal. Modals and notices draw over
/// either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// Sidebar, diff pane, and footer (or the splash when there is nothing
    /// to review).
    #[default]
    Review,
    /// GitHub pull requests to pick one to review.
    PullRequestList,
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
    loaded_files: Vec<FileEntry>,
    file_exclude_suffixes: ExcludeSuffixes,
    pub sidebar_items: Vec<SidebarItem>,
    pub collapsed_directories: HashSet<DirectoryKey>,
    pub collapsed_sections: HashSet<SidebarSection>,
    /// Generated files (lockfiles and similar) the user chose to show this
    /// session. Every other generated file renders as a placeholder.
    expanded_generated_files: HashSet<String>,
    pub sidebar_state: ListState,
    pub sidebar_scroll: usize,
    pub sidebar_viewport_height: usize,
    pub sidebar_hidden: bool,
    /// Last known mouse cell. Rendering derives hover highlights from it.
    pub mouse_position: Option<Position>,
    pub selected_sidebar_row: usize,
    pub selected_file_index: usize,
    pub diff_view: DiffView,
    pub diff_view_mode: DiffViewMode,
    pub diff_line_wrap_mode: DiffLineWrapMode,
    /// Whether diffs hide whitespace-only edits. Changes diff content, so it is
    /// part of every diff cache key and toggling it reloads the review.
    pub diff_whitespace_mode: git::WhitespaceMode,
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
    diff_prefetch_direction: DiffPrefetchDirection,
    diff_prefetch_anchor_file_index: Option<usize>,
    review_diff_stream_index: Option<ReviewDiffPartialTextIndex>,
    review_diff_text_index: Option<Arc<ReviewDiffTextIndex>>,
    review_diff_snapshot: Option<Arc<ReviewDiffSnapshot>>,
    review_diff_snapshot_request_id: u64,
    review_diff_snapshot_task: Option<task::JoinHandle<()>>,
    review_diff_stats: Option<git::ReviewDiffStats>,
    review_diff_stats_error: Option<String>,
    review_diff_stats_request_id: u64,
    review_diff_stats_task: Option<task::JoinHandle<()>>,
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
    pub diff_stats_modal_open: bool,
    pub help_modal_open: bool,
    pub theme_modal_open: bool,
    pub theme_modal_query: String,
    pub theme_modal_selected_index: usize,
    pub theme_modal_initial_name: String,
    pub theme_modal_initial_mode: ThemeMode,
    pub theme_name: String,
    pub theme_mode: ThemeMode,
    pub theme_matcher: Matcher,
    pub file_filter_modal_open: bool,
    pub file_filter_query: String,
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
    pub diff_search_mode: DiffSearchMode,
    diff_search_index: Option<Arc<DiffSearchIndex>>,
    diff_search_index_readiness: Option<DiffSearchIndexReadiness>,
    diff_search_index_error: Option<String>,
    diff_search_index_request_id: u64,
    diff_search_query_request_id: u64,
    diff_search_load_task: Option<task::JoinHandle<()>>,
    diff_search_query_task: Option<task::JoinHandle<()>>,
    diff_search_query_cancel_token: Option<Arc<AtomicBool>>,
    pending_diff_search_target: Option<DiffSearchNavigationTarget>,
    pending_change_landing: Option<PendingChangeLanding>,
    pub commit_search_modal_open: bool,
    pub commit_search_query: String,
    pub commit_search_entries: Vec<CommitSearchEntry>,
    pub commit_search_loading: bool,
    pub commit_search_error: Option<String>,
    pub commit_search_selected_index: usize,
    pub commit_search_matcher: Matcher,
    commit_search_index: ModalLookupIndex,
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
    branch_compare_ref_index: ModalLookupIndex,
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
    branch_status: BranchStatus,
    branch_panel: Option<BranchPanel>,
    branch_operation: Option<git::BranchOperation>,
    viewed_files: ViewedFiles,
    viewed_scope: Option<ViewedScope>,
    viewed_request_id: u64,
    screen: Screen,
    pull_requests: PullRequests,
    pub snackbar_notice: Option<SnackbarNotice>,
    pub snackbar_generation: u64,
    pub status_message: Option<String>,
}

#[cfg(test)]
mod tests;
