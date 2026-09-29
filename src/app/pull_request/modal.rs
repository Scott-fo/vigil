//! The review-action modals and the keys that open them.
//!
//! One [`PullRequestModal`] shows at a time, owned by the pull request
//! state so leaving the review closes it. The renderer reads a
//! [`PullRequestModalView`], which carries the modal and everything it
//! needs to draw (counts, warnings, blockers, whether a write is running);
//! it decides nothing itself.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    forge::{ForgeError, PullRequest},
    review::DraftComment,
};

use super::{
    super::{App, keyboard::KeyOutcome, text_area::TextArea},
    actions::ActionsMenu,
    composer::Composer,
    draft_list::DraftList,
    gateway::{ForgeMutation, MutationOutcome},
    merge::{AutoMergeChoice, MergeBlocker, MergeForm, auto_merge_choice, merge_blockers},
    state::ReviewOrigin,
    submit::{SubmitForm, SubmitWarning},
};

/// A review-action modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRequestModal {
    /// Boxed: it is far larger than the other modals.
    Composer(Box<Composer>),
    Submit(SubmitForm),
    Merge(MergeForm),
    Actions(ActionsMenu),
    Drafts(DraftList),
}

impl PullRequestModal {
    /// The text a modal edits, for "edit in $EDITOR".
    pub(in crate::app) fn text_mut(&mut self) -> Option<&mut TextArea> {
        match self {
            Self::Composer(composer) => Some(composer.text_mut()),
            Self::Submit(form) => Some(form.body_mut()),
            Self::Merge(_) | Self::Actions(_) | Self::Drafts(_) => None,
        }
    }
}

/// A draft as the modals and overview list it.
#[derive(Debug, Clone, Copy)]
pub struct DraftEntry<'a> {
    pub draft: &'a DraftComment,
    /// Whether its lines are at the reviewed head; detached drafts need
    /// attention and are not sent.
    pub attached: bool,
}

/// Everything a review-action modal draws.
#[derive(Debug)]
pub enum PullRequestModalView<'a> {
    Composer {
        composer: &'a Composer,
        /// A reply or comment is being posted.
        posting: bool,
    },
    Submit {
        form: &'a SubmitForm,
        number: u64,
        /// Drafts the review will send.
        draft_count: usize,
        is_author: bool,
        warnings: Vec<SubmitWarning>,
        submitting: bool,
    },
    Merge {
        form: &'a MergeForm,
        detail: &'a PullRequest,
        blockers: Vec<MergeBlocker>,
        auto_merge: AutoMergeChoice,
        /// The head the merge is pinned to.
        head_oid: &'a str,
        has_new_commits: bool,
        merging: bool,
    },
    Actions {
        menu: &'a ActionsMenu,
        number: u64,
        updating: bool,
    },
    Drafts {
        list: &'a DraftList,
        drafts: Vec<DraftEntry<'a>>,
    },
}

impl App {
    /// The review-action modal on screen, prepared for drawing.
    pub fn pull_request_modal(&self) -> Option<PullRequestModalView<'_>> {
        let open = self.pull_requests.open()?;
        let modal = self.pull_requests.modal()?;
        Some(match modal {
            PullRequestModal::Composer(composer) => PullRequestModalView::Composer {
                composer: composer.as_ref(),
                posting: self.composer_is_posting(),
            },
            PullRequestModal::Submit(form) => PullRequestModalView::Submit {
                form,
                number: open.summary().number,
                draft_count: open.drafts().attached(open.reviewed_head()).count(),
                is_author: open.detail().is_some_and(|detail| detail.viewer.is_author),
                warnings: self.submit_warnings(),
                submitting: self.submitting_review(),
            },
            PullRequestModal::Merge(form) => {
                let detail = open.detail()?;
                PullRequestModalView::Merge {
                    form,
                    detail,
                    blockers: merge_blockers(detail),
                    auto_merge: auto_merge_choice(detail),
                    head_oid: open.reviewed_head(),
                    has_new_commits: open.newer_head().is_some(),
                    merging: self.merging(),
                }
            }
            PullRequestModal::Actions(menu) => PullRequestModalView::Actions {
                menu,
                number: open.summary().number,
                updating: self.updating_state(),
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
            PullRequestModal::Submit(_) => self.handle_submit_key(key_event),
            PullRequestModal::Merge(_) => self.handle_merge_key(key_event),
            PullRequestModal::Actions(_) => self.handle_actions_key(key_event),
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
            KeyCode::Char('C') => self.start_conversation_comment(),
            KeyCode::Char('R') => self.start_thread_reply(),
            KeyCode::Char('T') => self.toggle_thread_resolved(),
            KeyCode::Char('S') => self.open_submit_review(),
            KeyCode::Char('M') => self.open_merge_form(),
            KeyCode::Char('A') => self.open_actions_menu(),
            KeyCode::Char('D') => self.open_draft_list(),
            KeyCode::Esc
                if self.diff_text_selection.is_none()
                    && self.pull_requests.open().map(|open| open.origin())
                        == Some(ReviewOrigin::PullRequestList) =>
            {
                self.return_to_pull_request_list().await?;
            }
            _ => return Ok(None),
        }
        Ok(Some(KeyOutcome::Handled))
    }

    /// Dispatches a finished write to the feature that started it.
    pub(super) fn handle_mutation_finished(
        &mut self,
        request_id: u64,
        result: Result<MutationOutcome, ForgeError>,
    ) -> bool {
        let Some(mutation) = self.pull_requests.finish_mutation(request_id) else {
            return false;
        };
        match mutation {
            ForgeMutation::SubmitReview { number, drafts, .. } => {
                self.finish_submit_review(number, drafts, result)
            }
            ForgeMutation::ReplyToThread { .. } | ForgeMutation::AddConversationComment { .. } => {
                self.finish_comment_post(result)
            }
            ForgeMutation::SetThreadResolved { .. } => self.finish_thread_resolve(result),
            ForgeMutation::Merge { number, .. } | ForgeMutation::DisableAutoMerge { number } => {
                self.finish_merge(number, result)
            }
            ForgeMutation::UpdateState { number, action } => {
                self.finish_state_update(number, action, result)
            }
        }
        true
    }
}
