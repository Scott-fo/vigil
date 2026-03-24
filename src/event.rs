use std::path::PathBuf;

use color_eyre::eyre::OptionExt;
use crossterm::event::Event as CrosstermEvent;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::watcher::RepoWatcher;
use crate::{
    app::DiffCacheKey,
    git::{BlameCommitDetails, CommitSearchEntry, DiffView, SharedHighlightRegistry},
};

#[derive(Debug)]
pub struct DiffPrefetchedEvent {
    pub generation: u64,
    pub key: DiffCacheKey,
    pub plain: DiffView,
    pub highlighted: Option<DiffView>,
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
    BlameLoaded {
        request_id: u64,
        result: Result<BlameCommitDetails, String>,
    },
    CommitSearchLoaded(Result<Vec<CommitSearchEntry>, String>),
    BranchCompareLoaded(Result<Vec<String>, String>),
    RepoWatcherReady(PathBuf, Result<RepoWatcher, String>),
    RepoChanged(Vec<PathBuf>),
    RemoteSyncFinished(Result<String, String>),
    ClearSnackbar(u64),
}

#[derive(Debug)]
pub struct EventHandler {
    sender: mpsc::UnboundedSender<Event>,
    receiver: mpsc::UnboundedReceiver<Event>,
    task: Option<JoinHandle<()>>,
}

impl Default for EventHandler {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let task = Some(spawn_event_task(sender.clone()));
        Self {
            sender,
            receiver,
            task,
        }
    }
}

impl EventHandler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<Event> {
        self.sender.clone()
    }

    pub fn suspend(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    pub fn resume(&mut self) {
        if self.task.is_none() {
            self.task = Some(spawn_event_task(self.sender.clone()));
        }
    }

    pub async fn next(&mut self) -> color_eyre::Result<Event> {
        self.receiver
            .recv()
            .await
            .ok_or_eyre("failed to receive event")
    }
}

struct EventTask {
    sender: mpsc::UnboundedSender<Event>,
}

fn spawn_event_task(sender: mpsc::UnboundedSender<Event>) -> JoinHandle<()> {
    let actor = EventTask::new(sender);
    tokio::spawn(async move {
        let _ = actor.run().await;
    })
}

impl EventTask {
    fn new(sender: mpsc::UnboundedSender<Event>) -> Self {
        Self { sender }
    }

    async fn run(self) -> color_eyre::Result<()> {
        let mut reader = crossterm::event::EventStream::new();

        while let Some(result) = reader.next().await {
            if let Ok(event) = result
                && !self.send(Event::Crossterm(event))
            {
                break;
            }
        }

        Ok(())
    }

    fn send(&self, event: Event) -> bool {
        self.sender.send(event).is_ok()
    }
}
