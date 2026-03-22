use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use notify::{Config, Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::event::Event;

const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

pub struct RepoWatcher {
    _watcher: RecommendedWatcher,
    debounce_task: JoinHandle<()>,
}

impl RepoWatcher {
    pub fn new(
        repo_root: PathBuf,
        event_sender: mpsc::UnboundedSender<Event>,
    ) -> notify::Result<Self> {
        let (signal_sender, signal_receiver) = mpsc::unbounded_channel();
        let mut watcher = RecommendedWatcher::new(
            move |result: notify::Result<NotifyEvent>| {
                if let Ok(event) = result {
                    if is_relevant_event(&event) {
                        let _ = signal_sender.send(event.paths);
                    }
                }
            },
            Config::default(),
        )?;
        watcher.watch(&repo_root, RecursiveMode::Recursive)?;

        let debounce_task = tokio::spawn(run_debounce_loop(signal_receiver, event_sender));

        Ok(Self {
            _watcher: watcher,
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

async fn run_debounce_loop(
    mut signal_receiver: mpsc::UnboundedReceiver<Vec<PathBuf>>,
    event_sender: mpsc::UnboundedSender<Event>,
) {
    while let Some(initial_paths) = signal_receiver.recv().await {
        let mut changed_paths = collect_paths(initial_paths);
        loop {
            let delay = tokio::time::sleep(WATCH_DEBOUNCE);
            tokio::pin!(delay);

            tokio::select! {
                _ = &mut delay => {
                    let _ = event_sender.send(Event::RepoChanged(changed_paths.into_iter().collect()));
                    break;
                }
                maybe_paths = signal_receiver.recv() => {
                    match maybe_paths {
                        Some(paths) => {
                            changed_paths.extend(collect_paths(paths));
                        }
                        None => {
                            let _ = event_sender.send(Event::RepoChanged(changed_paths.into_iter().collect()));
                            return;
                        }
                    }
                }
            }
        }
    }
}

fn is_relevant_event(event: &NotifyEvent) -> bool {
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

fn should_ignore_event_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized == ".git" || normalized.ends_with("/.git") || normalized.contains("/.git/")
}

fn collect_paths(paths: Vec<PathBuf>) -> HashSet<PathBuf> {
    paths.into_iter().collect()
}
