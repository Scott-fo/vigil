use std::time::Instant;

use tokio::task;

use crate::{
    event::Event,
    forge::{ForgeError, PullRequestSummary},
    git::HeadState,
};

use super::{
    super::{App, SnackbarVariant},
    PullRequestEvent,
    connect::forge_unavailable_message,
    state::{BranchKey, ConnectReason, PendingAction, ReviewOrigin},
};

impl App {
    /// The pull request for the checked-out branch, once looked up. Hidden
    /// while a different branch is checked out than the one it was found for.
    pub fn current_branch_pull_request(&self) -> Option<&PullRequestSummary> {
        let branch = self.current_branch_key()?.branch;
        self.pull_requests.current_branch_summary(&branch)
    }

    fn current_branch_key(&self) -> Option<BranchKey> {
        let snapshot = self.branch_snapshot()?;
        let HeadState::Branch(branch) = &snapshot.head else {
            return None;
        };
        Some(BranchKey {
            branch: branch.clone(),
            tip: snapshot
                .head_branch()
                .map(|entry| entry.tip.short_hash.clone())
                .unwrap_or_default(),
        })
    }

    /// Looks up the checked-out branch's pull request in the background.
    /// Connects to GitHub first if nothing has tried yet; stays quiet when
    /// GitHub is unavailable.
    pub(in crate::app) fn refresh_current_branch_pull_request(&mut self, force: bool) {
        let Some(key) = self.current_branch_key() else {
            self.pull_requests.clear_current_branch();
            return;
        };
        if !self.ensure_forge_connection(ConnectReason::Background) {
            return;
        }
        let Some(github) = self.pull_requests.github().cloned() else {
            return;
        };
        let Some(request_id) =
            self.pull_requests
                .begin_current_branch_load(key, force, Instant::now())
        else {
            return;
        };
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result = github.current_branch_pull_request().await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::CurrentBranchLoaded {
                request_id,
                result,
            }));
        });
        self.pull_requests.attach_current_branch(request_id, handle);
    }

    /// Opens the checked-out branch's pull request for review (the `O` key
    /// and the footer chip).
    pub(in crate::app) fn open_current_branch_pull_request(&mut self) {
        if self.current_branch_key().is_none() {
            self.show_snackbar(
                "check out a branch to open its pull request".to_string(),
                SnackbarVariant::Info,
            );
            return;
        }
        if let Some(summary) = self.current_branch_pull_request().cloned() {
            self.open_pull_request(summary, ReviewOrigin::Elsewhere);
            return;
        }
        self.pull_requests
            .set_pending(PendingAction::OpenCurrentBranch);
        if !self.request_forge_connection() {
            return;
        }
        self.status_message = Some("looking up this branch's pull request…".to_string());
        self.refresh_current_branch_pull_request(true);
    }

    pub(super) fn handle_current_branch_pull_request_loaded(
        &mut self,
        request_id: u64,
        result: Result<Option<PullRequestSummary>, ForgeError>,
    ) -> bool {
        if !self
            .pull_requests
            .finish_current_branch_load(request_id, &result, Instant::now())
        {
            return false;
        }
        if self.pull_requests.pending() != Some(PendingAction::OpenCurrentBranch) {
            // Opening the chip is the likeliest next step; have its detail
            // saved by then.
            if let Ok(Some(summary)) = &result {
                self.prefetch_current_branch_detail(summary);
            }
            return true;
        }
        self.pull_requests.take_pending();
        self.status_message = Some(self.current_status_message());
        match result {
            Ok(Some(summary)) => self.open_pull_request(summary, ReviewOrigin::Elsewhere),
            Ok(None) => {
                let branch = self
                    .current_branch_key()
                    .map(|key| key.branch)
                    .unwrap_or_default();
                self.show_snackbar(
                    format!("no pull request for {branch}"),
                    SnackbarVariant::Info,
                );
            }
            Err(error) => {
                self.show_snackbar(forge_unavailable_message(&error), SnackbarVariant::Error)
            }
        }
        true
    }
}
