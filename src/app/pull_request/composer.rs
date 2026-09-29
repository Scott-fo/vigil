//! The comment composer: one multi-line editor for every comment the
//! reviewer writes on a pull request.
//!
//! A [`Composer`] knows what its text is for ([`ComposerTarget`]). Saving a
//! draft stays local; saving a reply or a conversation comment posts it to
//! GitHub at once, and the composer stays open showing "posting…" until
//! GitHub answers, keeping the text if it fails.
//!
//! Inline comments come from the diff cursor, or from a drag selection
//! spanning lines. Only lines GitHub will accept qualify (see
//! [`DraftAnchor::from_patch_lines`]); with whitespace changes hidden vigil's
//! hunks are not GitHub's, so commenting is refused until they show again.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    forge::ThreadId,
    git::{DiffSelectionPane, PatchLine, WhitespaceMode},
    review::{DraftAnchor, DraftId, suggestion_block, suggestion_source},
};

use super::{
    super::{
        ActivePane, App, SnackbarVariant, editor::AppCommand, keyboard::KeyOutcome,
        text_area::TextArea,
    },
    gateway::{ForgeMutation, MutationOutcome},
    modal::PullRequestModal,
};

/// Shown when `c` is pressed while whitespace changes are hidden.
pub(super) const WHITESPACE_HIDDEN_HINT: &str =
    "comments need GitHub's diff: press W to show whitespace changes, then c";

/// What the composer's text becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerTarget {
    /// A new draft on `path`.
    NewDraft { path: String, anchor: DraftAnchor },
    /// An existing draft's body.
    EditDraft {
        id: DraftId,
        path: String,
        anchor: DraftAnchor,
    },
    /// A reply posted to a GitHub review thread.
    Reply {
        thread: ThreadId,
        path: String,
        line: Option<u32>,
    },
    /// A comment posted on the pull request's conversation.
    Conversation,
}

impl ComposerTarget {
    /// Whether saving posts to GitHub rather than storing a draft.
    pub fn posts_immediately(&self) -> bool {
        matches!(self, Self::Reply { .. } | Self::Conversation)
    }
}

/// Whether Esc asked to throw away edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposerStatus {
    Editing,
    /// Esc was pressed with unsaved text; Esc again discards it.
    ConfirmDiscard,
}

/// The comment being written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composer {
    target: ComposerTarget,
    text: TextArea,
    original: String,
    /// The selected new-side lines a suggestion block would propose.
    suggestion: Option<Vec<String>>,
    status: ComposerStatus,
    error: Option<String>,
}

impl Composer {
    pub(super) fn new(target: ComposerTarget, text: &str, suggestion: Option<Vec<String>>) -> Self {
        Self {
            target,
            text: TextArea::from_text(text),
            original: text.to_string(),
            suggestion,
            status: ComposerStatus::Editing,
            error: None,
        }
    }

    pub fn target(&self) -> &ComposerTarget {
        &self.target
    }

    pub fn text(&self) -> &TextArea {
        &self.text
    }

    pub(super) fn text_mut(&mut self) -> &mut TextArea {
        &mut self.text
    }

    pub fn can_suggest(&self) -> bool {
        self.suggestion.is_some()
    }

    pub fn status(&self) -> ComposerStatus {
        self.status
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(super) fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    fn is_dirty(&self) -> bool {
        self.text.text() != self.original
    }

    /// Inserts a suggestion block with the selected lines at the cursor.
    fn insert_suggestion(&mut self) {
        match &self.suggestion {
            Some(lines) => {
                let block = suggestion_block(lines);
                let (_, column) = self.text.cursor();
                if column > 0 {
                    self.text.newline();
                }
                self.text.insert_str(&block);
            }
            None => {
                self.error =
                    Some("suggestions replace new-side lines; this selection has none".to_string())
            }
        }
    }
}

impl App {
    /// `c` in pull request review: a draft on the selected lines, or on the
    /// overview a conversation comment.
    pub(in crate::app) fn start_inline_comment(&mut self) {
        if self.pull_request_overview_visible() {
            self.start_conversation_comment();
            return;
        }
        let Some(path) = self.selected_file().map(|file| file.path.clone()) else {
            self.show_snackbar(
                "select a file to comment on".to_string(),
                SnackbarVariant::Info,
            );
            return;
        };
        if self.diff_whitespace_mode == WhitespaceMode::Ignore {
            self.show_snackbar(WHITESPACE_HIDDEN_HINT.to_string(), SnackbarVariant::Info);
            return;
        }
        if self.selected_file_is_collapsed_generated() {
            self.show_snackbar(
                "show the generated diff (⏎) to comment on it".to_string(),
                SnackbarVariant::Info,
            );
            return;
        }
        if !self.diff_view.has_diff_rows() {
            self.show_snackbar(
                "this file's diff has no lines to comment on yet".to_string(),
                SnackbarVariant::Info,
            );
            return;
        }
        if self.active_pane != ActivePane::Diff && self.diff_text_selection.is_none() {
            self.show_snackbar(
                "focus the diff (tab) and pick a line to comment on".to_string(),
                SnackbarVariant::Info,
            );
            return;
        }
        let rows = self.selected_patch_rows();
        match DraftAnchor::from_patch_lines(&rows) {
            Ok(anchor) => {
                let suggestion = suggestion_source(&rows);
                self.open_composer(Composer::new(
                    ComposerTarget::NewDraft { path, anchor },
                    "",
                    suggestion,
                ));
            }
            Err(refusal) => self.show_snackbar(refusal.to_string(), SnackbarVariant::Info),
        }
    }

    /// A comment on the pull request's conversation (`C`, or `c` on the
    /// overview).
    pub(in crate::app) fn start_conversation_comment(&mut self) {
        if self.pull_requests.open().is_none() {
            return;
        }
        self.open_composer(Composer::new(ComposerTarget::Conversation, "", None));
    }

    /// Opens a draft in the composer to change its text.
    pub(in crate::app) fn edit_draft(&mut self, id: &DraftId) {
        let Some(draft) = self
            .pull_requests
            .open()
            .and_then(|open| open.drafts().get(id))
            .cloned()
        else {
            return;
        };
        self.open_composer(Composer::new(
            ComposerTarget::EditDraft {
                id: draft.id,
                path: draft.path,
                anchor: draft.anchor,
            },
            &draft.body,
            None,
        ));
    }

    pub(super) fn open_composer(&mut self, composer: Composer) {
        self.pull_requests
            .set_modal(Some(PullRequestModal::Composer(Box::new(composer))));
    }

    /// The patch line of every selected row: the drag selection's rows, or
    /// the cursor's. Rows outside the patch are `None`. In split view a drag
    /// selects one side; rows that show only the other side are skipped.
    fn selected_patch_rows(&mut self) -> Vec<Option<PatchLine>> {
        let mode = self.diff_view_mode;
        let width = self.current_diff_display_width();
        let wrap = self.diff_line_wrap_mode;
        let (range, pane) = match self.diff_text_selection {
            Some(selection) => {
                let (a, b) = (selection.anchor.display_index, selection.head.display_index);
                (a.min(b)..=a.max(b), selection.anchor.pane)
            }
            None => (
                self.selected_diff_line_index..=self.selected_diff_line_index,
                DiffSelectionPane::Unified,
            ),
        };
        let mut rows = Vec::new();
        for index in range {
            match self.diff_view.patch_line_at(mode, width, wrap, index, pane) {
                Some(line) => rows.push(Some(line)),
                None if pane != DiffSelectionPane::Unified
                    && self
                        .diff_view
                        .patch_line_at(mode, width, wrap, index, DiffSelectionPane::Unified)
                        .is_some() => {}
                None => rows.push(None),
            }
        }
        rows
    }

    /// Whether the composer's post to GitHub is running.
    pub(in crate::app) fn composer_is_posting(&self) -> bool {
        matches!(
            self.pull_requests.in_flight_mutation(),
            Some(
                ForgeMutation::ReplyToThread { .. } | ForgeMutation::AddConversationComment { .. }
            )
        ) && matches!(
            self.pull_requests.modal(),
            Some(PullRequestModal::Composer(_))
        )
    }

    pub(super) fn handle_composer_key(&mut self, key_event: KeyEvent) -> Option<KeyOutcome> {
        if self.composer_is_posting() {
            return Some(KeyOutcome::Handled);
        }
        let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
        let Some(PullRequestModal::Composer(composer)) = self.pull_requests.modal_mut() else {
            return None;
        };
        match key_event.code {
            KeyCode::Char('s') if control => {
                self.save_composer();
                return Some(KeyOutcome::Handled);
            }
            KeyCode::Char('e') if control => {
                return Some(KeyOutcome::Command(AppCommand::EditPullRequestText));
            }
            KeyCode::Char('g') if control => composer.insert_suggestion(),
            KeyCode::Esc => {
                if composer.is_dirty() && composer.status != ComposerStatus::ConfirmDiscard {
                    composer.status = ComposerStatus::ConfirmDiscard;
                } else {
                    self.pull_requests.set_modal(None);
                }
                return Some(KeyOutcome::Handled);
            }
            _ => {
                if composer.text.handle_key(key_event) {
                    composer.error = None;
                }
            }
        }
        if let Some(PullRequestModal::Composer(composer)) = self.pull_requests.modal_mut() {
            composer.status = ComposerStatus::Editing;
        }
        Some(KeyOutcome::Handled)
    }

    /// Ctrl-S: stores a draft, or posts a reply or conversation comment.
    fn save_composer(&mut self) {
        let Some(PullRequestModal::Composer(composer)) = self.pull_requests.modal_mut() else {
            return;
        };
        if composer.text.is_blank() {
            composer.error = Some("the comment is empty".to_string());
            return;
        }
        let body = composer.text.text().trim_end().to_string();
        match composer.target.clone() {
            ComposerTarget::NewDraft { path, anchor } => {
                let Some(draft) = self.new_draft(path, anchor, body) else {
                    return;
                };
                self.save_draft(draft);
                self.pull_requests.set_modal(None);
                self.clear_diff_text_selection();
                self.show_snackbar(
                    "draft saved · S submits your review".to_string(),
                    SnackbarVariant::Info,
                );
            }
            ComposerTarget::EditDraft { id, .. } => {
                let draft = self
                    .pull_requests
                    .open()
                    .and_then(|open| open.drafts().get(&id))
                    .cloned();
                self.pull_requests.set_modal(None);
                if let Some(mut draft) = draft {
                    draft.set_body(body);
                    self.save_draft(draft);
                    self.show_snackbar("draft updated".to_string(), SnackbarVariant::Info);
                }
            }
            ComposerTarget::Reply { thread, .. } => {
                self.start_forge_mutation(ForgeMutation::ReplyToThread { thread, body });
            }
            ComposerTarget::Conversation => {
                let Some(number) = self.pull_requests.open_number() else {
                    return;
                };
                self.start_forge_mutation(ForgeMutation::AddConversationComment { number, body });
            }
        }
    }

    /// A reply or conversation comment finished posting.
    pub(super) fn finish_comment_post(
        &mut self,
        result: Result<MutationOutcome, crate::forge::ForgeError>,
    ) {
        match result {
            Ok(outcome) => {
                if matches!(
                    self.pull_requests.modal(),
                    Some(PullRequestModal::Composer(_))
                ) {
                    self.pull_requests.set_modal(None);
                }
                let message = match outcome {
                    MutationOutcome::Replied => "reply posted",
                    _ => "comment posted",
                };
                self.show_snackbar(message.to_string(), SnackbarVariant::Info);
                self.reload_after_mutation(false);
            }
            Err(error) => {
                let message = format!("GitHub rejected the comment: {error}");
                if let Some(PullRequestModal::Composer(composer)) = self.pull_requests.modal_mut() {
                    composer.set_error(message.clone());
                }
                self.show_snackbar(message, SnackbarVariant::Error);
            }
        }
    }
}
