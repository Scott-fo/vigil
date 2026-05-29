use tokio::task;

use crate::{event::Event, git};

use super::super::{App, RemoteSyncDirection};

impl App {
    pub(in crate::app) fn start_push(&mut self) {
        if self.remote_sync.is_some() {
            return;
        }

        self.remote_sync = Some(RemoteSyncDirection::Push);
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::push_to_remote(&repo_root)
                .await
                .map(|_| "Pushed to remote".to_string())
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::RemoteSyncFinished(result));
        }));
    }

    pub(in crate::app) fn start_pull(&mut self) {
        if self.remote_sync.is_some() {
            return;
        }

        self.remote_sync = Some(RemoteSyncDirection::Pull);
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::pull_from_remote(&repo_root)
                .await
                .map(|_| "Pulled from remote".to_string())
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::RemoteSyncFinished(result));
        }));
    }
}
