use tokio::task;

use crate::{
    event::Event,
    git::{self, BranchSnapshot},
};

use super::super::App;

/// The latest branch snapshot and the load that will replace it.
#[derive(Debug, Default)]
pub(in crate::app) struct BranchStatus {
    snapshot: Option<BranchSnapshot>,
    error: Option<String>,
    loading: bool,
    pub(super) request_id: u64,
}

impl BranchStatus {
    pub(super) fn snapshot(&self) -> Option<&BranchSnapshot> {
        self.snapshot.as_ref()
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn loading(&self) -> bool {
        self.loading
    }

    #[cfg(test)]
    pub(super) fn set_snapshot(&mut self, snapshot: BranchSnapshot) {
        self.snapshot = Some(snapshot);
        self.error = None;
        self.loading = false;
    }
}

impl App {
    /// Branch facts from the most recent load, if one has finished.
    pub fn branch_snapshot(&self) -> Option<&BranchSnapshot> {
        self.branch_status.snapshot()
    }

    #[cfg(test)]
    pub(crate) fn set_branch_snapshot(&mut self, snapshot: BranchSnapshot) {
        self.branch_status.set_snapshot(snapshot);
    }

    pub(in crate::app) fn queue_branch_status_load(&mut self) {
        self.branch_status.request_id = self.branch_status.request_id.saturating_add(1);
        self.branch_status.loading = true;
        let request_id = self.branch_status.request_id;
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::load_branch_snapshot(&repo_root)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::BranchStatusLoaded { request_id, result });
        }));
    }

    pub(in crate::app) fn handle_branch_status_loaded(
        &mut self,
        request_id: u64,
        result: Result<BranchSnapshot, String>,
    ) -> bool {
        if request_id != self.branch_status.request_id {
            return false;
        }

        self.branch_status.loading = false;
        match result {
            Ok(snapshot) => {
                self.branch_status.snapshot = Some(snapshot);
                self.branch_status.error = None;
            }
            Err(error) => {
                self.branch_status.snapshot = None;
                self.branch_status.error = Some(error);
            }
        }
        if let (Some(panel), Some(snapshot)) = (
            self.branch_panel.as_mut(),
            self.branch_status.snapshot.as_ref(),
        ) {
            panel.seed_selection(snapshot);
        }
        self.refresh_current_branch_pull_request(false);
        true
    }
}
