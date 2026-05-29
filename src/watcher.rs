use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use notify::{Config, Event as NotifyEvent, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{sync::mpsc, task, task::JoinHandle};

use crate::event::Event;

use self::{
    debounce::run_debounce_loop,
    discovery::collect_watch_directories,
    filter::{is_relevant_event, maybe_watch_new_directories},
};

mod debounce;
mod discovery;
mod filter;

pub struct RepoWatcher {
    _watcher: Arc<Mutex<Option<RecommendedWatcher>>>,
    debounce_task: JoinHandle<()>,
}

impl RepoWatcher {
    pub async fn initialize(
        repo_root: PathBuf,
        event_sender: mpsc::UnboundedSender<Event>,
    ) -> Result<Self, String> {
        let watch_dirs = collect_watch_directories(&repo_root).await?;
        task::spawn_blocking(move || Self::from_watch_dirs(watch_dirs, event_sender))
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
    }

    fn from_watch_dirs(
        watch_dirs: Vec<PathBuf>,
        event_sender: mpsc::UnboundedSender<Event>,
    ) -> notify::Result<Self> {
        let (signal_sender, signal_receiver) = mpsc::unbounded_channel();
        let watcher_ref: Arc<Mutex<Option<RecommendedWatcher>>> = Arc::new(Mutex::new(None));
        let watcher_ref_for_callback = watcher_ref.clone();

        let watcher = RecommendedWatcher::new(
            move |result: notify::Result<NotifyEvent>| {
                if let Ok(event) = result
                    && is_relevant_event(&event)
                {
                    maybe_watch_new_directories(&watcher_ref_for_callback, &event.paths);
                    let _ = signal_sender.send(event.paths);
                }
            },
            Config::default(),
        )?;

        {
            let mut guard = watcher_ref.lock().expect("repo watcher mutex poisoned");
            *guard = Some(watcher);
            if let Some(watcher) = guard.as_mut() {
                for watch_dir in &watch_dirs {
                    watcher.watch(watch_dir, RecursiveMode::NonRecursive)?;
                }
            }
        }

        let debounce_task = tokio::spawn(run_debounce_loop(signal_receiver, event_sender));

        Ok(Self {
            _watcher: watcher_ref,
            debounce_task,
        })
    }
}

impl Drop for RepoWatcher {
    fn drop(&mut self) {
        self.debounce_task.abort();
    }
}

impl std::fmt::Debug for RepoWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoWatcher").finish_non_exhaustive()
    }
}
