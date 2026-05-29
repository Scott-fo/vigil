use std::path::PathBuf;

use tokio::{fs, task};

use crate::{event::Event, watcher::RepoWatcher};

use super::super::App;

impl App {
    pub(in crate::app) fn spawn_repo_watcher_init(&mut self) {
        if self.repo_watcher_loading {
            return;
        }

        self.repo_watcher_loading = true;
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = RepoWatcher::initialize(repo_root.clone(), sender.clone()).await;
            let _ = sender.send(Event::RepoWatcherReady(repo_root, result));
        }));
    }

    pub(in crate::app) fn restart_repo_watcher(&mut self) {
        self.repo_watcher = None;
        self.repo_watcher_loading = false;
        self.spawn_repo_watcher_init();
    }

    pub(in crate::app) async fn should_restart_watcher_for_paths(&self, paths: &[PathBuf]) -> bool {
        for path in paths {
            if path
                .file_name()
                .is_some_and(|file_name| file_name == ".gitignore")
            {
                return true;
            }

            if let Ok(metadata) = fs::metadata(path).await
                && metadata.is_dir()
            {
                return true;
            }
        }

        false
    }
}
