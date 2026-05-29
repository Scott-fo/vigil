use tokio::task;

use crate::{event::Event, git};

use super::super::{ActivePane, App, ReviewMode};

impl App {
    pub(in crate::app) fn open_branch_merge_modal(&mut self) {
        let ReviewMode::BranchCompare(selection) = &self.review_mode else {
            self.status_message = Some("merge is available from branch compare mode".to_string());
            return;
        };

        self.branch_merge_target = Some(git::BranchMergeRequest {
            source_ref: selection.source_ref.clone(),
            destination_ref: selection.destination_ref.clone(),
        });
        self.branch_merge_loading = false;
        self.branch_merge_error = None;
    }

    pub(in crate::app) fn close_branch_merge_modal(&mut self) {
        if self.branch_merge_loading {
            return;
        }

        self.branch_merge_target = None;
        self.branch_merge_error = None;
    }

    pub(in crate::app) fn confirm_branch_merge(&mut self) {
        if self.branch_merge_loading {
            return;
        }

        let Some(target) = self.branch_merge_target.clone() else {
            return;
        };

        self.branch_merge_loading = true;
        self.branch_merge_error = None;
        self.status_message = Some(format!(
            "merging {} into {}...",
            target.source_ref, target.destination_ref
        ));

        let repo_root = self.repo_root.clone();
        let sender = self.events.sender();
        self.track_background_task(task::spawn(async move {
            let result = git::prepare_branch_merge(&repo_root, &target)
                .await
                .map_err(|error| error.to_string());
            let _ = sender.send(Event::BranchMergeFinished(result));
        }));
    }

    pub(in crate::app) async fn handle_branch_merge_finished(
        &mut self,
        result: Result<git::BranchMergeOutcome, String>,
    ) -> color_eyre::Result<()> {
        self.branch_merge_loading = false;
        match result {
            Ok(outcome) => {
                self.branch_merge_target = None;
                self.branch_merge_error = None;
                let status_message = branch_merge_status_message(&outcome);
                let conflicted = matches!(outcome, git::BranchMergeOutcome::Conflicted { .. });
                self.review_mode = ReviewMode::WorkingTree;
                self.refresh().await?;
                if conflicted && !self.files.is_empty() {
                    self.active_pane = ActivePane::Diff;
                }
                self.status_message = Some(status_message);
            }
            Err(error) => {
                self.branch_merge_error = Some(error.clone());
                self.status_message = Some(format!("merge failed: {error}"));
            }
        }
        Ok(())
    }
}

fn branch_merge_status_message(outcome: &git::BranchMergeOutcome) -> String {
    match outcome {
        git::BranchMergeOutcome::Prepared {
            source_ref,
            destination_ref,
        } => format!("prepared merge of {source_ref} into {destination_ref}"),
        git::BranchMergeOutcome::Conflicted {
            source_ref,
            destination_ref,
        } => format!("merge conflict: {source_ref} into {destination_ref}"),
        git::BranchMergeOutcome::AlreadyUpToDate {
            source_ref,
            destination_ref,
        } => format!("{destination_ref} is already up to date with {source_ref}"),
    }
}
