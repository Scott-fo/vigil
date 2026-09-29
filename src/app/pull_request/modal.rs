//! The review-action modals and the keys that open them.
//!
//! One [`PullRequestModal`] shows at a time, owned by the pull request
//! state so leaving the review closes it. The renderer reads a
//! [`PullRequestModalView`], which carries the modal and everything it
//! needs to draw; it decides nothing itself.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::review::DraftComment;

use super::{
    super::{App, keyboard::KeyOutcome, text_area::TextArea},
    composer::Composer,
    draft_list::DraftList,
};

/// A review-action modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRequestModal {
    /// Boxed: it is far larger than the other modals.
    Composer(Box<Composer>),
    Drafts(DraftList),
}

impl PullRequestModal {
    /// The text a modal edits, for "edit in $EDITOR".
    pub(in crate::app) fn text_mut(&mut self) -> Option<&mut TextArea> {
        match self {
            Self::Composer(composer) => Some(composer.text_mut()),
            Self::Drafts(_) => None,
        }
    }
}

/// A draft as the modals and overview list it.
#[derive(Debug, Clone, Copy)]
pub struct DraftEntry<'a> {
    pub draft: &'a DraftComment,
    /// Whether its lines are at the reviewed head; detached drafts need
    /// attention.
    pub attached: bool,
}

/// Everything a review-action modal draws.
#[derive(Debug)]
pub enum PullRequestModalView<'a> {
    Composer {
        composer: &'a Composer,
    },
    Drafts {
        list: &'a DraftList,
        drafts: Vec<DraftEntry<'a>>,
    },
}

impl App {
    /// The review-action modal on screen, prepared for drawing.
    pub fn pull_request_modal(&self) -> Option<PullRequestModalView<'_>> {
        self.pull_requests.open()?;
        Some(match self.pull_requests.modal()? {
            PullRequestModal::Composer(composer) => PullRequestModalView::Composer {
                composer: composer.as_ref(),
            },
            PullRequestModal::Drafts(list) => PullRequestModalView::Drafts {
                list,
                drafts: self.draft_entries(),
            },
        })
    }

    /// Every draft of the open pull request, oldest first.
    pub fn draft_entries(&self) -> Vec<DraftEntry<'_>> {
        let Some(open) = self.pull_requests.open() else {
            return Vec::new();
        };
        let head = open.reviewed_head();
        open.drafts()
            .all()
            .iter()
            .map(|draft| DraftEntry {
                draft,
                attached: draft.head_oid == head,
            })
            .collect()
    }

    pub(in crate::app) fn pull_request_modal_open(&self) -> bool {
        self.pull_requests.modal().is_some()
    }

    /// The text the open review modal edits, if it edits any.
    pub(in crate::app) fn pull_request_modal_text(&mut self) -> Option<&mut TextArea> {
        self.pull_requests.modal_mut()?.text_mut()
    }

    /// Keys for the open review-action modal; `None` when none is open.
    pub(in crate::app) fn handle_pull_request_modal_key(
        &mut self,
        key_event: KeyEvent,
    ) -> Option<KeyOutcome> {
        match self.pull_requests.modal()? {
            PullRequestModal::Composer(_) => self.handle_composer_key(key_event),
            PullRequestModal::Drafts(_) => self.handle_draft_list_key(key_event),
        }
        .or(Some(KeyOutcome::Handled))
    }

    /// Review keys while a pull request is under review. Returns `None` for
    /// keys that are not review actions, which keep their usual meaning.
    pub(in crate::app) async fn handle_pull_request_review_key(
        &mut self,
        key_event: KeyEvent,
    ) -> color_eyre::Result<Option<KeyOutcome>> {
        if self.pull_request_selection().is_none()
            || key_event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Ok(None);
        }
        match key_event.code {
            KeyCode::Char('c') => self.start_inline_comment(),
            KeyCode::Char('D') => self.open_draft_list(),
            _ => return Ok(None),
        }
        Ok(Some(KeyOutcome::Handled))
    }
}
