use tokio::task;

use crate::{
    event::Event,
    git::{self, BranchOperation, BranchOperationError, BranchOperationOutcome},
};

use super::super::{App, SnackbarVariant};
use super::BranchPanelMode;

impl App {
    /// The git branch operation in flight, if any.
    pub fn branch_operation(&self) -> Option<&BranchOperation> {
        self.branch_operation.as_ref()
    }

    /// Runs `operation` in the background unless another one is running.
    /// Returns whether it started.
    pub(in crate::app) fn start_branch_operation(&mut self, operation: BranchOperation) -> bool {
        if self.branch_operation.is_some() {
            self.status_message = Some("a git operation is already running".to_string());
            return false;
        }

        self.branch_operation = Some(operation.clone());
        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::run_branch_operation(&repo_root, &operation).await;
            let _ = sender.send(Event::BranchOperationFinished(result));
        }));
        true
    }

    /// Applies a finished operation: reloads what it invalidated and reports
    /// the result in the panel when it is open, otherwise as a notice.
    pub(in crate::app) async fn handle_branch_operation_finished(
        &mut self,
        result: Result<BranchOperationOutcome, BranchOperationError>,
    ) -> color_eyre::Result<()> {
        self.branch_operation = None;
        match result {
            Ok(outcome) => {
                if matches!(
                    outcome,
                    BranchOperationOutcome::Switched { .. }
                        | BranchOperationOutcome::Created { .. }
                ) {
                    self.close_branch_panel();
                }
                self.show_snackbar(outcome_message(&outcome), SnackbarVariant::Info);
                if outcome.changes_working_tree() {
                    // Refreshing the review also reloads the branch snapshot.
                    self.refresh().await?;
                    return Ok(());
                }
            }
            Err(BranchOperationError::NotFullyMerged { branch }) if self.branch_panel.is_some() => {
                if let Some(panel) = self.branch_panel.as_mut() {
                    panel.set_mode(BranchPanelMode::Delete {
                        branch,
                        force: true,
                    });
                }
            }
            Err(error) => match self.branch_panel.as_mut() {
                Some(panel) => panel.set_error(error.to_string()),
                None => self.show_snackbar(error.to_string(), SnackbarVariant::Error),
            },
        }
        self.queue_branch_status_load();
        Ok(())
    }
}

fn outcome_message(outcome: &BranchOperationOutcome) -> String {
    match outcome {
        BranchOperationOutcome::Fetched => "Fetched from remotes".to_string(),
        BranchOperationOutcome::Pulled {
            branch,
            up_to_date: true,
        } => format!("{branch} is already up to date"),
        BranchOperationOutcome::Pulled { branch, .. } => format!("Pulled into {branch}"),
        BranchOperationOutcome::Pushed {
            upstream,
            upstream_created: true,
        } => format!("Published to {upstream}"),
        BranchOperationOutcome::Pushed { upstream, .. } => format!("Pushed to {upstream}"),
        BranchOperationOutcome::Switched { branch } => format!("Switched to {branch}"),
        BranchOperationOutcome::Created { branch } => format!("Created and switched to {branch}"),
        BranchOperationOutcome::Renamed { from, to } => format!("Renamed {from} to {to}"),
        BranchOperationOutcome::Deleted { branch } => format!("Deleted {branch}"),
    }
}
