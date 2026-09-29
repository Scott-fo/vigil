//! The drafts list (`D`): every pending comment on the pull request, to edit
//! or delete, including drafts that lost their lines to new commits.

use crossterm::event::{KeyCode, KeyEvent};

use super::{
    super::{App, SnackbarVariant, keyboard::KeyOutcome},
    modal::PullRequestModal,
};

/// Selection in the drafts list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DraftList {
    selected: usize,
    /// `d` was pressed once; `d` again deletes the selected draft.
    confirm_delete: bool,
}

impl DraftList {
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn confirm_delete(&self) -> bool {
        self.confirm_delete
    }
}

impl App {
    /// `D`: lists the drafts.
    pub(in crate::app) fn open_draft_list(&mut self) {
        let Some(open) = self.pull_requests.open() else {
            return;
        };
        if open.drafts().all().is_empty() {
            self.show_snackbar(
                "no draft comments yet; press c on a diff line to write one".to_string(),
                SnackbarVariant::Info,
            );
            return;
        }
        self.pull_requests
            .set_modal(Some(PullRequestModal::Drafts(DraftList::default())));
    }

    pub(super) fn handle_draft_list_key(&mut self, key_event: KeyEvent) -> Option<KeyOutcome> {
        let count = self.pull_requests.open()?.drafts().all().len();
        let Some(PullRequestModal::Drafts(list)) = self.pull_requests.modal_mut() else {
            return None;
        };
        if count == 0 {
            self.pull_requests.set_modal(None);
            return Some(KeyOutcome::Handled);
        }
        list.selected = list.selected.min(count - 1);
        let armed = std::mem::take(&mut list.confirm_delete);
        match key_event.code {
            KeyCode::Esc => self.pull_requests.set_modal(None),
            KeyCode::Down | KeyCode::Char('j') => list.selected = (list.selected + 1) % count,
            KeyCode::Up | KeyCode::Char('k') => list.selected = (list.selected + count - 1) % count,
            KeyCode::Enter | KeyCode::Char('e') => {
                let index = list.selected;
                let id = self.pull_requests.open()?.drafts().all()[index].id.clone();
                self.edit_draft(&id);
            }
            KeyCode::Char('d') if armed => {
                let index = list.selected;
                let id = self.pull_requests.open()?.drafts().all()[index].id.clone();
                self.delete_drafts(vec![id]);
                self.show_snackbar("draft deleted".to_string(), SnackbarVariant::Info);
                if count == 1 {
                    self.pull_requests.set_modal(None);
                }
            }
            KeyCode::Char('d') => list.confirm_delete = true,
            _ => {}
        }
        Some(KeyOutcome::Handled)
    }
}
