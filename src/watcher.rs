use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use notify::{Config, Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{io::AsyncWriteExt, process::Command, sync::mpsc, task, task::JoinHandle};

use crate::event::Event;

const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

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
                if let Ok(event) = result {
                    if is_relevant_event(&event) {
                        maybe_watch_new_directories(&watcher_ref_for_callback, &event.paths);
                        let _ = signal_sender.send(event.paths);
                    }
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

async fn collect_watch_directories(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let visible_paths = git_visible_paths(repo_root).await?;
    let mut directories = HashSet::from([repo_root.to_path_buf()]);

    for path in visible_paths {
        let mut current = repo_root.join(path);
        while let Some(parent) = current.parent() {
            if !parent.starts_with(repo_root) {
                break;
            }
            directories.insert(parent.to_path_buf());
            if parent == repo_root {
                break;
            }
            current = parent.to_path_buf();
        }
    }

    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort();
    Ok(directories)
}

async fn git_visible_paths(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = git_output(
        repo_root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
        None,
    )
    .await?;

    Ok(output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect())
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

fn maybe_watch_new_directories(
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
        if let Ok(metadata) = std::fs::metadata(path) {
            if metadata.is_dir() {
                let _ = watcher.watch(path, RecursiveMode::NonRecursive);
            }
        }
    }
}

fn should_ignore_event_path(path: &Path) -> bool {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized == ".git" || normalized.ends_with("/.git") || normalized.contains("/.git/")
}

fn collect_paths(paths: Vec<PathBuf>) -> HashSet<PathBuf> {
    paths.into_iter().collect()
}

async fn git_output(
    repo_root: &Path,
    args: &[&str],
    stdin: Option<&[u8]>,
) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_root).args(args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command.spawn().map_err(|error| error.to_string())?;

    if let (Some(input), Some(mut child_stdin)) = (stdin, child.stdin.take()) {
        child_stdin
            .write_all(input)
            .await
            .map_err(|error| error.to_string())?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
