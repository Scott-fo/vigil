use tokio::task;

use crate::{
    event::Event,
    forge::{ForgeError, GitHub},
};

use super::{
    super::{App, SnackbarVariant},
    PullRequestEvent,
    state::{ConnectOutcome, ConnectReason, ForgeConnection, PendingAction},
};

const CONNECTING_MESSAGE: &str = "connecting to GitHub…";

impl App {
    /// Probes GitHub in the background if nothing has yet. Returns whether a
    /// connected client is available now.
    pub(in crate::app) fn ensure_forge_connection(&mut self, reason: ConnectReason) -> bool {
        self.pull_requests.rebind_repo_root(&self.repo_root);
        if self.pull_requests.github().is_some() {
            return true;
        }
        if let Some(request_id) = self.pull_requests.begin_connect(reason) {
            let repo_root = self.repo_root.clone();
            let sender = self.events.sender();
            let handle = task::spawn(async move {
                let result = GitHub::connect(repo_root).await;
                let _ = sender.send(Event::PullRequest(PullRequestEvent::Connected {
                    request_id,
                    result,
                }));
            });
            self.pull_requests.attach_connect(request_id, handle);
        }
        false
    }

    /// Answers an explicit request for a pull request feature while GitHub is
    /// not connected: starts connecting, or says why it cannot.
    pub(in crate::app) fn request_forge_connection(&mut self) -> bool {
        if self.ensure_forge_connection(ConnectReason::UserRequest) {
            return true;
        }
        match self.pull_requests.connection() {
            ForgeConnection::Disabled => {
                self.show_snackbar(
                    "pull requests are unavailable here".to_string(),
                    SnackbarVariant::Error,
                );
            }
            ForgeConnection::Unavailable(error) => {
                let message = forge_unavailable_message(error);
                self.show_snackbar(message, SnackbarVariant::Error);
            }
            ForgeConnection::Idle | ForgeConnection::Connecting => {
                self.status_message = Some(CONNECTING_MESSAGE.to_string());
            }
            ForgeConnection::Connected(_) => return true,
        }
        false
    }

    pub(super) fn handle_forge_connected(
        &mut self,
        request_id: u64,
        result: Result<GitHub, ForgeError>,
    ) -> bool {
        let Some(outcome) = self.pull_requests.finish_connect(request_id, result) else {
            return false;
        };
        match outcome {
            ConnectOutcome::Connected => {
                self.pull_requests.rebind_repo_root(&self.repo_root);
                let user_waiting =
                    self.pull_requests.pending() == Some(PendingAction::OpenCurrentBranch);
                self.refresh_current_branch_pull_request(user_waiting);
                self.load_pull_request_list_if_shown();
                if self.status_message.as_deref() == Some(CONNECTING_MESSAGE) {
                    self.status_message = Some(self.current_status_message());
                }
            }
            ConnectOutcome::Failed { report } => {
                if let Some(error) = report {
                    self.show_snackbar(forge_unavailable_message(&error), SnackbarVariant::Error);
                    self.status_message = Some(self.current_status_message());
                }
            }
        }
        true
    }
}

/// A one-line explanation of why pull request features are off.
pub(super) fn forge_unavailable_message(error: &ForgeError) -> String {
    match error {
        ForgeError::NotGitHubRepository { .. } => {
            "pull requests need a GitHub remote; this repository has none".to_string()
        }
        other => format!("pull requests are unavailable: {other}"),
    }
}
