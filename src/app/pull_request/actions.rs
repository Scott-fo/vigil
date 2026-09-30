//! The pull request actions menu: close or reopen, and move between draft
//! and ready for review.
//!
//! The menu lists only what the viewer may do right now, from the
//! pull request's state and [`Viewer`](crate::forge::Viewer) capabilities.
//! Closing asks for a second ⏎; the other actions are easily undone and run
//! at once.

use crossterm::event::{KeyCode, KeyEvent};

use crate::forge::{ForgeError, PullRequest, PullRequestState};

use super::{
    super::{App, SnackbarVariant, keyboard::KeyOutcome},
    gateway::{ForgeMutation, MutationOutcome},
    modal::PullRequestModal,
};

/// A change to the pull request's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PullRequestAction {
    Close,
    Reopen,
    MarkReadyForReview,
    ConvertToDraft,
}

impl PullRequestAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Close => "Close pull request",
            Self::Reopen => "Reopen pull request",
            Self::MarkReadyForReview => "Mark ready for review",
            Self::ConvertToDraft => "Convert to draft",
        }
    }

    /// Whether running it needs a second, confirming key press.
    pub fn needs_confirmation(self) -> bool {
        matches!(self, Self::Close)
    }

    fn done_message(self, number: u64) -> String {
        match self {
            Self::Close => format!("closed #{number}"),
            Self::Reopen => format!("reopened #{number}"),
            Self::MarkReadyForReview => format!("#{number} is ready for review"),
            Self::ConvertToDraft => format!("#{number} is a draft again"),
        }
    }
}

/// The actions the viewer may take on `detail` now, in menu order.
pub fn available_actions(detail: &PullRequest) -> Vec<PullRequestAction> {
    let viewer = &detail.viewer;
    let summary = &detail.summary;
    let mut actions = Vec::new();
    match summary.state {
        PullRequestState::Open => {
            if viewer.can_update {
                actions.push(if summary.is_draft {
                    PullRequestAction::MarkReadyForReview
                } else {
                    PullRequestAction::ConvertToDraft
                });
            }
            if viewer.can_close {
                actions.push(PullRequestAction::Close);
            }
        }
        PullRequestState::Closed => {
            if viewer.can_reopen {
                actions.push(PullRequestAction::Reopen);
            }
        }
        PullRequestState::Merged => {}
    }
    actions
}

/// The actions menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsMenu {
    actions: Vec<PullRequestAction>,
    selected: usize,
    /// The action waiting for a confirming ⏎.
    confirming: Option<PullRequestAction>,
    error: Option<String>,
}

impl ActionsMenu {
    pub fn actions(&self) -> &[PullRequestAction] {
        &self.actions
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn confirming(&self) -> Option<PullRequestAction> {
        self.confirming
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl App {
    /// `A`: opens the actions menu, if the viewer may do anything.
    pub(in crate::app) fn open_actions_menu(&mut self) {
        let Some(open) = self.pull_requests.open() else {
            return;
        };
        let number = open.summary().number;
        // State changes are offered from what GitHub says now, never from a
        // saved snapshot.
        if let Some(blocker) = self.unconfirmed_detail_blocker() {
            self.show_snackbar(blocker.to_string(), SnackbarVariant::Info);
            return;
        }
        let Some(detail) = open.live_detail() else {
            self.show_snackbar(
                "pull request details are still loading".to_string(),
                SnackbarVariant::Info,
            );
            return;
        };
        let actions = available_actions(detail);
        if actions.is_empty() {
            self.show_snackbar(
                format!("GitHub allows you no state changes on #{number}"),
                SnackbarVariant::Info,
            );
            return;
        }
        self.pull_requests
            .set_modal(Some(PullRequestModal::Actions(ActionsMenu {
                actions,
                selected: 0,
                confirming: None,
                error: None,
            })));
    }

    pub(super) fn updating_state(&self) -> bool {
        matches!(
            self.pull_requests.in_flight_mutation(),
            Some(ForgeMutation::UpdateState { .. })
        )
    }

    pub(super) fn handle_actions_key(&mut self, key_event: KeyEvent) -> Option<KeyOutcome> {
        if self.updating_state() {
            return Some(KeyOutcome::Handled);
        }
        let number = self.pull_requests.open_number()?;
        let Some(PullRequestModal::Actions(menu)) = self.pull_requests.modal_mut() else {
            return None;
        };
        menu.error = None;
        let count = menu.actions.len();
        match key_event.code {
            KeyCode::Esc if menu.confirming.is_some() => menu.confirming = None,
            KeyCode::Esc => self.pull_requests.set_modal(None),
            KeyCode::Down | KeyCode::Char('j') => {
                menu.confirming = None;
                menu.selected = (menu.selected + 1) % count;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                menu.confirming = None;
                menu.selected = (menu.selected + count - 1) % count;
            }
            KeyCode::Enter => {
                let action = menu.actions[menu.selected];
                if action.needs_confirmation() && menu.confirming != Some(action) {
                    menu.confirming = Some(action);
                } else {
                    self.start_forge_mutation(ForgeMutation::UpdateState { number, action });
                }
            }
            _ => {}
        }
        Some(KeyOutcome::Handled)
    }

    /// GitHub answered a state change.
    pub(super) fn finish_state_update(
        &mut self,
        number: u64,
        action: PullRequestAction,
        result: Result<MutationOutcome, ForgeError>,
    ) {
        match result {
            Ok(_) => {
                if matches!(
                    self.pull_requests.modal(),
                    Some(PullRequestModal::Actions(_))
                ) {
                    self.pull_requests.set_modal(None);
                }
                self.show_snackbar(action.done_message(number), SnackbarVariant::Info);
                self.reload_after_mutation(true);
            }
            Err(error) => {
                let message = format!("could not {}: {error}", action.label().to_lowercase());
                if let Some(PullRequestModal::Actions(menu)) = self.pull_requests.modal_mut() {
                    menu.confirming = None;
                    menu.error = Some(message.clone());
                }
                self.show_snackbar(message, SnackbarVariant::Error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::pull_request_fixtures as fixtures;

    #[test]
    fn actions_follow_state_and_viewer_capabilities() {
        let mut detail = fixtures::pull_request(18, Vec::new());
        assert!(available_actions(&detail).is_empty(), "no capabilities");

        detail.viewer.can_update = true;
        detail.viewer.can_close = true;
        assert_eq!(
            available_actions(&detail),
            vec![PullRequestAction::ConvertToDraft, PullRequestAction::Close]
        );

        detail.summary.is_draft = true;
        assert_eq!(
            available_actions(&detail)[0],
            PullRequestAction::MarkReadyForReview
        );

        detail.summary.state = PullRequestState::Closed;
        assert!(
            available_actions(&detail).is_empty(),
            "reopen needs can_reopen"
        );
        detail.viewer.can_reopen = true;
        assert_eq!(available_actions(&detail), vec![PullRequestAction::Reopen]);

        detail.summary.state = PullRequestState::Merged;
        assert!(available_actions(&detail).is_empty());
    }

    #[test]
    fn only_closing_asks_for_confirmation() {
        assert!(PullRequestAction::Close.needs_confirmation());
        for action in [
            PullRequestAction::Reopen,
            PullRequestAction::MarkReadyForReview,
            PullRequestAction::ConvertToDraft,
        ] {
            assert!(!action.needs_confirmation());
        }
    }
}
