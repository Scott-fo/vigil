use crate::ui::splash;

use super::super::{App, ReviewMode};

impl App {
    pub fn show_splash(&self) -> bool {
        self.repo_error.is_some() || (self.is_working_tree_mode() && self.files.is_empty())
    }

    pub fn splash_error(&self) -> Option<&str> {
        self.repo_error.as_deref()
    }

    pub fn can_initialize_git_repo(&self) -> bool {
        self.repo_error
            .as_deref()
            .is_some_and(splash::is_not_git_repository_error)
    }

    pub fn review_mode_label(&self) -> String {
        match &self.review_mode {
            ReviewMode::WorkingTree => String::new(),
            ReviewMode::CommitCompare(selection) => {
                format!("Commit {}: {}", selection.short_hash, selection.subject)
            }
            ReviewMode::BranchCompare(selection) => {
                format!(
                    "Compare {} -> {}",
                    selection.source_ref, selection.destination_ref
                )
            }
        }
    }

    pub(crate) fn current_status_message(&self) -> String {
        self.repo_error
            .clone()
            .unwrap_or_else(|| self.default_status_message())
    }

    pub fn shows_review_summary_status(&self) -> bool {
        match self.status_message.as_deref() {
            None => true,
            Some(message) => message == self.default_status_message(),
        }
    }

    pub(crate) fn default_status_message(&self) -> String {
        match &self.review_mode {
            ReviewMode::WorkingTree => format!(
                "{} changed file{}",
                self.files.len(),
                if self.files.len() == 1 { "" } else { "s" }
            ),
            ReviewMode::CommitCompare(selection) => {
                format!(
                    "commit {}  {} file{}",
                    selection.short_hash,
                    self.files.len(),
                    if self.files.len() == 1 { "" } else { "s" }
                )
            }
            ReviewMode::BranchCompare(selection) => {
                format!(
                    "{} -> {}  {} file{}",
                    selection.source_ref,
                    selection.destination_ref,
                    self.files.len(),
                    if self.files.len() == 1 { "" } else { "s" }
                )
            }
        }
    }
}
