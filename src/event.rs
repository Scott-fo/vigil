use std::path::PathBuf;

use crossterm::event::Event as CrosstermEvent;

use crate::{
    app::DiffCacheKey,
    git::{
        BlameCommitDetails, BranchCompareRefs, BranchMergeOutcome, CommitSearchEntry, DiffView,
        SharedHighlightRegistry, WorkingTreeStatus, WorktreeEntry,
    },
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
    WorkingTreeStatusLoaded {
        request_id: u64,
        result: Result<WorkingTreeStatus, String>,
    },
    BlameLoaded {
        request_id: u64,
        result: Result<BlameCommitDetails, String>,
    },
    CommitSearchLoaded(Result<Vec<CommitSearchEntry>, String>),
    BranchCompareLoaded(Result<BranchCompareRefs, String>),
    BranchMergeFinished(Result<BranchMergeOutcome, String>),
    WorktreesLoaded(Result<Vec<WorktreeEntry>, String>),
    RepoWatcherReady(PathBuf, Result<RepoWatcher, String>),
    RepoChanged(Vec<PathBuf>),
    RemoteSyncFinished(Result<String, String>),
    ClearSnackbar(u64),
}
