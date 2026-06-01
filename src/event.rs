use std::{path::PathBuf, sync::Arc};

use crossterm::event::Event as CrosstermEvent;

use crate::{
    app::DiffCacheKey,
    git::{
        BlameCommitDetails, BranchCompareRefs, BranchMergeOutcome, CommitSearchEntry,
        DiffSearchIndex, DiffSearchResults, DiffView, ReviewDiffSnapshot, ReviewDiffStats,
        ReviewDiffStreamedFile, ReviewDiffTextIndex, SharedHighlightRegistry, WorkingTreeStatus,
        WorktreeEntry,
    },
    review::PersistedReview,
    watcher::RepoWatcher,
};

mod handler;
mod task;

pub use self::handler::EventHandler;

#[derive(Debug)]
pub struct DiffPrefetchedEvent {
    pub generation: u64,
    pub key: DiffCacheKey,
    pub plain: DiffView,
    pub highlighted: Option<DiffView>,
    pub highlight_complete: bool,
}

#[derive(Debug)]
pub enum Event {
    Crossterm(CrosstermEvent),
    HighlightRegistryReady(Result<SharedHighlightRegistry, String>),
    DiffLoaded {
        request_id: u64,
        result: Result<DiffView, String>,
    },
    DiffHighlightUpdated {
        request_id: u64,
        complete: bool,
        result: Result<DiffView, String>,
    },
    DiffPrefetched(Box<DiffPrefetchedEvent>),
    ReviewDiffSnapshotLoaded {
        request_id: u64,
        generation: u64,
        result: Result<ReviewDiffSnapshot, String>,
    },
    ReviewDiffTextIndexLoaded {
        request_id: u64,
        generation: u64,
        result: Result<Arc<ReviewDiffTextIndex>, String>,
    },
    ReviewDiffFileStreamed {
        request_id: u64,
        generation: u64,
        file: ReviewDiffStreamedFile,
    },
    ReviewDiffStatsLoaded {
        request_id: u64,
        generation: u64,
        result: Result<ReviewDiffStats, String>,
    },
    WorkingTreeStatusLoaded {
        request_id: u64,
        result: Result<WorkingTreeStatus, String>,
    },
    BlameLoaded {
        request_id: u64,
        result: Result<BlameCommitDetails, String>,
    },
    CommitSearchLoaded(Result<Vec<CommitSearchEntry>, String>),
    DiffSearchIndexLoaded {
        request_id: u64,
        result: Result<DiffSearchIndex, String>,
    },
    DiffSearchResultsLoaded {
        request_id: u64,
        result: Result<DiffSearchResults, String>,
    },
    BranchCompareLoaded(Result<BranchCompareRefs, String>),
    BranchMergeFinished(Result<BranchMergeOutcome, String>),
    WorktreesLoaded(Result<Vec<WorktreeEntry>, String>),
    ReviewFinished {
        request_id: u64,
        result: Result<PersistedReview, String>,
    },
    ReviewLoaded {
        request_id: u64,
        result: Result<Option<PersistedReview>, String>,
    },
    RepoWatcherReady(PathBuf, Result<RepoWatcher, String>),
    RepoChanged(Vec<PathBuf>),
    RemoteSyncFinished(Result<String, String>),
    ClearSnackbar(u64),
}
