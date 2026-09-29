//! Submitting a review: a verdict, an optional summary, and the drafts.
//!
//! The review is pinned to the head the drafts were written against, which
//! is the head the review shows; drafts that could not follow the pull
//! request to that head stay behind. The form enforces GitHub's rules before
//! anything is sent: authors cannot approve or request changes on their own
//! pull request, and a comment or change request needs a summary or at
//! least one inline comment. A pending review started in GitHub's web UI
//! and newer commits than the reviewed head are warned about, not blocked.
//! On success the sent drafts are deleted; on failure they stay and the
//! form shows GitHub's error.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    forge::{ForgeError, ReviewEvent, SubmitReview},
    review::DraftComment,
};

use super::{
    super::{App, SnackbarVariant, editor::AppCommand, keyboard::KeyOutcome, text_area::TextArea},
    gateway::{ForgeMutation, MutationOutcome},
    modal::PullRequestModal,
};

/// Every verdict, in the order the form offers them.
pub const REVIEW_EVENTS: [ReviewEvent; 3] = [
    ReviewEvent::Comment,
    ReviewEvent::Approve,
    ReviewEvent::RequestChanges,
];

/// Why the review cannot be submitted as it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitBlocker {
    /// GitHub rejects approving or requesting changes on your own pull
    /// request.
    OwnPullRequest(ReviewEvent),
    /// A comment or change request needs a summary or inline comments.
    NothingToSay(ReviewEvent),
    /// The pull request's details (and so the viewer's permissions) have not
    /// loaded yet.
    DetailsLoading,
}

impl fmt::Display for SubmitBlocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnPullRequest(event) => write!(
                formatter,
                "you cannot {} your own pull request",
                match event {
                    ReviewEvent::RequestChanges => "request changes on",
                    _ => "approve",
                }
            ),
            Self::NothingToSay(ReviewEvent::RequestChanges) => {
                formatter.write_str("requesting changes needs a summary or a draft comment")
            }
            Self::NothingToSay(_) => {
                formatter.write_str("a comment review needs a summary or a draft comment")
            }
            Self::DetailsLoading => formatter.write_str("pull request details are still loading"),
        }
    }
}

/// Something the reviewer should know before submitting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitWarning {
    /// A review started in GitHub's web UI is still pending, and GitHub
    /// refuses to create another until it is submitted or deleted there.
    PendingReviewOnGitHub { comment_count: u32 },
    /// The pull request moved past the reviewed head; the review and its
    /// comments attach to the reviewed head and may show as outdated.
    NewCommits { reviewed: String, latest: String },
    /// Drafts written against another head whose lines were not found.
    DraftsLeftBehind(usize),
}

/// Whether `event` is available to the viewer.
pub fn event_allowed(event: ReviewEvent, is_author: bool) -> bool {
    !is_author || event == ReviewEvent::Comment
}

/// The request for a review of `head` with `drafts`, or why it cannot go.
pub(in crate::app) fn build_review(
    event: ReviewEvent,
    body: &str,
    drafts: &[&DraftComment],
    head: &str,
    is_author: bool,
) -> Result<SubmitReview, SubmitBlocker> {
    if !event_allowed(event, is_author) {
        return Err(SubmitBlocker::OwnPullRequest(event));
    }
    let body = body.trim_end();
    if event != ReviewEvent::Approve && body.trim().is_empty() && drafts.is_empty() {
        return Err(SubmitBlocker::NothingToSay(event));
    }
    Ok(SubmitReview {
        commit_oid: head.to_string(),
        event,
        body: body.to_string(),
        comments: drafts
            .iter()
            .map(|draft| draft.to_review_comment())
            .collect(),
    })
}

/// The submit-review form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitForm {
    event: ReviewEvent,
    body: TextArea,
    error: Option<String>,
}

impl SubmitForm {
    pub(super) fn new() -> Self {
        Self {
            event: ReviewEvent::Comment,
            body: TextArea::default(),
            error: None,
        }
    }

    pub fn event(&self) -> ReviewEvent {
        self.event
    }

    pub fn body(&self) -> &TextArea {
        &self.body
    }

    pub(super) fn body_mut(&mut self) -> &mut TextArea {
        &mut self.body
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Moves to the next verdict the viewer may pick.
    fn cycle_event(&mut self, delta: isize, is_author: bool) {
        let allowed = REVIEW_EVENTS
            .iter()
            .copied()
            .filter(|event| event_allowed(*event, is_author))
            .collect::<Vec<_>>();
        let current = allowed
            .iter()
            .position(|event| *event == self.event)
            .unwrap_or(0) as isize;
        let next = (current + delta).rem_euclid(allowed.len() as isize) as usize;
        self.event = allowed[next];
        self.error = None;
    }
}

impl App {
    /// `S`: opens the submit-review form.
    pub(in crate::app) fn open_submit_review(&mut self) {
        let Some(open) = self.pull_requests.open() else {
            return;
        };
        if open.detail().is_none() {
            self.show_snackbar(
                SubmitBlocker::DetailsLoading.to_string(),
                SnackbarVariant::Info,
            );
            return;
        }
        self.pull_requests
            .set_modal(Some(PullRequestModal::Submit(SubmitForm::new())));
    }

    /// Whether the viewer authored the open pull request.
    fn viewer_is_author(&self) -> bool {
        self.pull_requests
            .open()
            .and_then(|open| open.detail())
            .is_some_and(|detail| detail.viewer.is_author)
    }

    /// Warnings the submit form shows, most important first.
    pub(in crate::app) fn submit_warnings(&self) -> Vec<SubmitWarning> {
        let Some(open) = self.pull_requests.open() else {
            return Vec::new();
        };
        let mut warnings = Vec::new();
        if let Some(pending) = open
            .detail()
            .and_then(|detail| detail.viewer.pending_review.as_ref())
        {
            warnings.push(SubmitWarning::PendingReviewOnGitHub {
                comment_count: pending.comment_count,
            });
        }
        if let Some(latest) = open.newer_head() {
            warnings.push(SubmitWarning::NewCommits {
                reviewed: open.reviewed_head().to_string(),
                latest: latest.to_string(),
            });
        }
        let left_behind = open
            .drafts()
            .needing_attention(open.reviewed_head())
            .count();
        if left_behind > 0 {
            warnings.push(SubmitWarning::DraftsLeftBehind(left_behind));
        }
        warnings
    }

    pub(super) fn submitting_review(&self) -> bool {
        matches!(
            self.pull_requests.in_flight_mutation(),
            Some(ForgeMutation::SubmitReview { .. })
        )
    }

    pub(super) fn handle_submit_key(&mut self, key_event: KeyEvent) -> Option<KeyOutcome> {
        if self.submitting_review() {
            return Some(KeyOutcome::Handled);
        }
        let is_author = self.viewer_is_author();
        let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
        let Some(PullRequestModal::Submit(form)) = self.pull_requests.modal_mut() else {
            return None;
        };
        match key_event.code {
            KeyCode::Esc => self.pull_requests.set_modal(None),
            KeyCode::Tab => form.cycle_event(1, is_author),
            KeyCode::BackTab => form.cycle_event(-1, is_author),
            KeyCode::Char('s') if control => self.submit_review(),
            KeyCode::Char('e') if control => {
                return Some(KeyOutcome::Command(AppCommand::EditPullRequestText));
            }
            _ => {
                if form.body.handle_key(key_event) {
                    form.error = None;
                }
            }
        }
        Some(KeyOutcome::Handled)
    }

    /// Ctrl-S in the submit form.
    fn submit_review(&mut self) {
        let Some(open) = self.pull_requests.open() else {
            return;
        };
        let Some(PullRequestModal::Submit(form)) = self.pull_requests.modal() else {
            return;
        };
        let number = open.summary().number;
        let head = open.reviewed_head().to_string();
        let drafts = open.drafts().attached(&head).collect::<Vec<_>>();
        let draft_ids = drafts.iter().map(|draft| draft.id.clone()).collect();
        let result = match open.detail() {
            None => Err(SubmitBlocker::DetailsLoading),
            Some(detail) => build_review(
                form.event,
                &form.body.text(),
                &drafts,
                &head,
                detail.viewer.is_author,
            ),
        };
        match result {
            Ok(review) => {
                self.start_forge_mutation(ForgeMutation::SubmitReview {
                    number,
                    review,
                    drafts: draft_ids,
                });
            }
            Err(blocker) => {
                if let Some(PullRequestModal::Submit(form)) = self.pull_requests.modal_mut() {
                    form.error = Some(blocker.to_string());
                }
            }
        }
    }

    /// GitHub answered a submitted review.
    pub(super) fn finish_submit_review(
        &mut self,
        number: u64,
        drafts: Vec<crate::review::DraftId>,
        result: Result<MutationOutcome, ForgeError>,
    ) {
        match result {
            Ok(_) => {
                let count = drafts.len();
                self.delete_submitted_drafts(number, drafts);
                if matches!(
                    self.pull_requests.modal(),
                    Some(PullRequestModal::Submit(_))
                ) {
                    self.pull_requests.set_modal(None);
                }
                let comments = match count {
                    0 => String::new(),
                    1 => " with 1 comment".to_string(),
                    count => format!(" with {count} comments"),
                };
                self.show_snackbar(
                    format!("review submitted on #{number}{comments}"),
                    SnackbarVariant::Info,
                );
                self.reload_after_mutation(false);
            }
            Err(error) => {
                let message = format!("GitHub rejected the review; your drafts are kept: {error}");
                if let Some(PullRequestModal::Submit(form)) = self.pull_requests.modal_mut() {
                    form.error = Some(message.clone());
                }
                self.show_snackbar(message, SnackbarVariant::Error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        forge::{DiffPosition, DiffSide},
        review::{DraftAnchor, DraftLine},
    };

    const HEAD: &str = "0123456789012345678901234567890123456789";

    fn draft() -> DraftComment {
        DraftComment::new(
            "src/lib.rs".to_string(),
            DraftAnchor {
                start: None,
                end: DraftLine {
                    position: DiffPosition {
                        side: DiffSide::Right,
                        line: 3,
                    },
                    text: String::new(),
                },
            },
            "Consider a guard.".to_string(),
            HEAD.to_string(),
        )
    }

    #[test]
    fn comments_and_change_requests_need_a_body_or_drafts() {
        let draft = draft();
        for event in [ReviewEvent::Comment, ReviewEvent::RequestChanges] {
            assert_eq!(
                build_review(event, "  \n", &[], HEAD, false),
                Err(SubmitBlocker::NothingToSay(event))
            );
            assert!(build_review(event, "Looks close.", &[], HEAD, false).is_ok());
            assert!(build_review(event, "", &[&draft], HEAD, false).is_ok());
        }
        let approval = build_review(ReviewEvent::Approve, "", &[], HEAD, false).unwrap();
        assert!(approval.body.is_empty() && approval.comments.is_empty());
    }

    #[test]
    fn authors_can_only_comment() {
        for event in [ReviewEvent::Approve, ReviewEvent::RequestChanges] {
            assert!(!event_allowed(event, true));
            assert_eq!(
                build_review(event, "LGTM", &[], HEAD, true),
                Err(SubmitBlocker::OwnPullRequest(event))
            );
        }
        assert!(build_review(ReviewEvent::Comment, "Note", &[], HEAD, true).is_ok());

        let mut form = SubmitForm::new();
        form.cycle_event(1, true);
        assert_eq!(form.event(), ReviewEvent::Comment, "nothing else to pick");
        form.cycle_event(1, false);
        assert_eq!(form.event(), ReviewEvent::Approve);
        form.cycle_event(-2, false);
        assert_eq!(form.event(), ReviewEvent::RequestChanges);
    }

    #[test]
    fn reviews_pin_to_the_drafts_head_and_carry_every_draft() {
        let draft = draft();
        let review = build_review(
            ReviewEvent::RequestChanges,
            "Please fix.\n",
            &[&draft],
            HEAD,
            false,
        )
        .unwrap();

        assert_eq!(review.commit_oid, HEAD);
        assert_eq!(review.body, "Please fix.");
        assert_eq!(review.comments, vec![draft.to_review_comment()]);
    }
}
