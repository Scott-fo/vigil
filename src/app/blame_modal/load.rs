use tokio::task;

use crate::{
    event::Event,
    git::{self, BlameTarget},
};

use super::super::App;

impl App {
    pub(in crate::app) fn open_blame_target(&mut self, target: BlameTarget) {
        self.cancel_inflight_blame_load();
        self.blame_modal_open = true;
        self.blame_target = Some(target.clone());
        self.blame_loading = true;
        self.blame_details = None;
        self.blame_error = None;
        self.blame_scroll = 0;
        self.blame_request_id = self.blame_request_id.saturating_add(1);
        let request_id = self.blame_request_id;

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.blame_load_task = Some(task::spawn(async move {
            let result = git::load_blame_commit_details(&repo_root, &target)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::BlameLoaded { request_id, result });
        }));
    }

    pub(in crate::app) fn close_blame_modal(&mut self) {
        self.cancel_inflight_blame_load();
        self.blame_modal_open = false;
        self.blame_loading = false;
        self.blame_target = None;
        self.blame_details = None;
        self.blame_error = None;
        self.blame_scroll = 0;
    }

    pub(in crate::app) fn cancel_inflight_blame_load(&mut self) {
        if let Some(task) = self.blame_load_task.take() {
            task.abort();
        }
    }
}
