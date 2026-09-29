//! Merging the pull request under review.
//!
//! The merge form starts from the repository's settings: only methods the
//! repository allows, the viewer's last method preselected, and head-branch
//! deletion defaulting to the repository's "delete on merge". It lists what
//! stands in the way in words, offers auto-merge ("merge when ready") when
//! the repository allows it and the pull request is blocked or waiting on
//! checks, and offers to turn auto-merge off when it is on. Every merge is
//! pinned to the head the review shows, so GitHub refuses it if the branch
//! moved, and needs a second confirming key press.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent};

use crate::forge::{
    CheckState, ForgeError, HeadBranchAction, HeadBranchOutcome, MergeMethod, MergeOptions,
    MergeOutcome, MergeStateStatus, MergeTiming, Mergeability, PullRequest, PullRequestState,
    ReviewDecision,
};

use super::{
    super::{App, SnackbarVariant, keyboard::KeyOutcome},
    gateway::{ForgeMutation, MutationOutcome},
    modal::PullRequestModal,
};

/// When the merge happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeWhen {
    Now,
    /// Enable auto-merge; GitHub merges once requirements pass.
    WhenReady,
}

/// Where the merge form is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStep {
    Choose,
    /// ⏎ again merges (or enables auto-merge).
    ConfirmMerge,
    /// ⏎ again turns auto-merge off.
    ConfirmDisableAutoMerge,
}

/// Something standing between the pull request and a merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeBlocker {
    Closed,
    Merged,
    Draft,
    Conflicts,
    /// Branch protection blocks it (reviews, required checks).
    BranchProtection,
    /// Protection requires the head to be up to date with the base.
    Behind,
    ChangesRequested,
    ReviewRequired,
    ChecksFailing(u32),
    ChecksPending(u32),
    /// GitHub is still computing mergeability.
    Unknown,
}

impl MergeBlocker {
    /// Blockers no merge attempt can get past.
    pub fn is_final(self) -> bool {
        matches!(self, Self::Closed | Self::Merged | Self::Draft)
    }
}

impl fmt::Display for MergeBlocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => formatter.write_str("the pull request is closed"),
            Self::Merged => formatter.write_str("the pull request is already merged"),
            Self::Draft => {
                formatter.write_str("draft pull requests cannot merge; mark it ready (A)")
            }
            Self::Conflicts => formatter.write_str("conflicts with the base branch"),
            Self::BranchProtection => formatter.write_str("blocked by branch protection"),
            Self::Behind => formatter.write_str("the head is behind the base branch"),
            Self::ChangesRequested => formatter.write_str("changes were requested"),
            Self::ReviewRequired => formatter.write_str("an approving review is required"),
            Self::ChecksFailing(count) => write!(
                formatter,
                "{count} check{} failing",
                if *count == 1 { " is" } else { "s are" }
            ),
            Self::ChecksPending(count) => write!(
                formatter,
                "{count} check{} still running",
                if *count == 1 { " is" } else { "s are" }
            ),
            Self::Unknown => formatter.write_str("GitHub is still computing mergeability"),
        }
    }
}

/// Everything that stands in the way of merging `detail` now.
pub fn merge_blockers(detail: &PullRequest) -> Vec<MergeBlocker> {
    let mut blockers = Vec::new();
    match detail.summary.state {
        PullRequestState::Closed => return vec![MergeBlocker::Closed],
        PullRequestState::Merged => return vec![MergeBlocker::Merged],
        PullRequestState::Open => {}
    }
    if detail.summary.is_draft || detail.merge_state == MergeStateStatus::Draft {
        blockers.push(MergeBlocker::Draft);
    }
    if detail.mergeable == Mergeability::Conflicting
        || detail.merge_state == MergeStateStatus::Dirty
    {
        blockers.push(MergeBlocker::Conflicts);
    }
    match detail.summary.review_decision {
        Some(ReviewDecision::ChangesRequested) => blockers.push(MergeBlocker::ChangesRequested),
        Some(ReviewDecision::ReviewRequired) => blockers.push(MergeBlocker::ReviewRequired),
        Some(ReviewDecision::Approved) | None => {}
    }
    if let Some(checks) = detail.summary.checks {
        if checks.counts.failure > 0 {
            blockers.push(MergeBlocker::ChecksFailing(checks.counts.failure));
        }
        if checks.counts.pending > 0 {
            blockers.push(MergeBlocker::ChecksPending(checks.counts.pending));
        }
    }
    match detail.merge_state {
        MergeStateStatus::Behind => blockers.push(MergeBlocker::Behind),
        MergeStateStatus::Blocked if blockers.is_empty() => {
            blockers.push(MergeBlocker::BranchProtection)
        }
        MergeStateStatus::Unknown if blockers.is_empty() => blockers.push(MergeBlocker::Unknown),
        _ => {}
    }
    blockers
}

/// What the form can do about auto-merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoMergeChoice {
    /// Not allowed here, or nothing to wait for.
    Unavailable,
    /// "Merge when ready" can be picked.
    Offered,
    /// Auto-merge is on; `can_disable` says whether the viewer may turn it
    /// off.
    Enabled { can_disable: bool },
}

pub fn auto_merge_choice(detail: &PullRequest) -> AutoMergeChoice {
    if detail.auto_merge.is_some() {
        return AutoMergeChoice::Enabled {
            can_disable: detail.viewer.can_disable_auto_merge,
        };
    }
    let waiting = detail.merge_state == MergeStateStatus::Blocked
        || detail
            .summary
            .checks
            .is_some_and(|checks| checks.state == CheckState::Pending || checks.counts.pending > 0);
    if detail.summary.state == PullRequestState::Open
        && detail.merge_settings.auto_merge_allowed
        && detail.viewer.can_enable_auto_merge
        && waiting
    {
        AutoMergeChoice::Offered
    } else {
        AutoMergeChoice::Unavailable
    }
}

/// The merge form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeForm {
    method: MergeMethod,
    head_branch: HeadBranchAction,
    when: MergeWhen,
    step: MergeStep,
    error: Option<String>,
}

impl MergeForm {
    /// Defaults from the repository's settings: the viewer's last method if
    /// still allowed, and deleting the head branch when the repository
    /// deletes merged branches.
    pub fn new(detail: &PullRequest) -> Self {
        let settings = &detail.merge_settings;
        let method = if settings
            .allowed_methods
            .contains(&settings.viewer_default_method)
        {
            settings.viewer_default_method
        } else {
            settings
                .allowed_methods
                .first()
                .copied()
                .unwrap_or(settings.viewer_default_method)
        };
        Self {
            method,
            head_branch: if settings.delete_branch_on_merge {
                HeadBranchAction::Delete
            } else {
                HeadBranchAction::Keep
            },
            when: MergeWhen::Now,
            step: MergeStep::Choose,
            error: None,
        }
    }

    pub fn method(&self) -> MergeMethod {
        self.method
    }

    pub fn head_branch(&self) -> HeadBranchAction {
        self.head_branch
    }

    pub fn when(&self) -> MergeWhen {
        self.when
    }

    pub fn step(&self) -> MergeStep {
        self.step
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn cycle_method(&mut self, detail: &PullRequest, delta: isize) {
        let allowed = &detail.merge_settings.allowed_methods;
        if allowed.is_empty() {
            return;
        }
        let current = allowed
            .iter()
            .position(|method| *method == self.method)
            .unwrap_or(0) as isize;
        self.method = allowed[(current + delta).rem_euclid(allowed.len() as isize) as usize];
    }

    /// The request for this form, pinned to `expected_head_oid`.
    pub fn options(&self, expected_head_oid: &str) -> MergeOptions {
        MergeOptions {
            method: self.method,
            expected_head_oid: expected_head_oid.to_string(),
            timing: match self.when {
                MergeWhen::Now => MergeTiming::Now {
                    head_branch: self.head_branch,
                },
                MergeWhen::WhenReady => MergeTiming::WhenReady,
            },
            commit_message: None,
        }
    }
}

impl App {
    /// `M`: opens the merge form.
    pub(in crate::app) fn open_merge_form(&mut self) {
        let Some(detail) = self.pull_requests.open().and_then(|open| open.detail()) else {
            self.show_snackbar(
                "pull request details are still loading".to_string(),
                SnackbarVariant::Info,
            );
            return;
        };
        let form = MergeForm::new(detail);
        self.pull_requests
            .set_modal(Some(PullRequestModal::Merge(form)));
    }

    pub(super) fn merging(&self) -> bool {
        matches!(
            self.pull_requests.in_flight_mutation(),
            Some(ForgeMutation::Merge { .. } | ForgeMutation::DisableAutoMerge { .. })
        )
    }

    pub(super) fn handle_merge_key(&mut self, key_event: KeyEvent) -> Option<KeyOutcome> {
        if self.merging() {
            return Some(KeyOutcome::Handled);
        }
        let open = self.pull_requests.open()?;
        let detail = open.detail()?.clone();
        let number = open.summary().number;
        let head = open.reviewed_head().to_string();
        let Some(PullRequestModal::Merge(form)) = self.pull_requests.modal_mut() else {
            return None;
        };
        let auto_merge = auto_merge_choice(&detail);
        form.error = None;
        match (form.step, key_event.code) {
            (MergeStep::Choose, KeyCode::Esc) => self.pull_requests.set_modal(None),
            (_, KeyCode::Esc) => form.step = MergeStep::Choose,
            (MergeStep::Choose, KeyCode::Left | KeyCode::Char('h')) => {
                form.cycle_method(&detail, -1)
            }
            (MergeStep::Choose, KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab) => {
                form.cycle_method(&detail, 1)
            }
            (MergeStep::Choose, KeyCode::Char('d')) => {
                form.head_branch = match form.head_branch {
                    HeadBranchAction::Keep => HeadBranchAction::Delete,
                    HeadBranchAction::Delete => HeadBranchAction::Keep,
                };
            }
            (MergeStep::Choose, KeyCode::Char('w')) => match auto_merge {
                AutoMergeChoice::Offered => {
                    form.when = match form.when {
                        MergeWhen::Now => MergeWhen::WhenReady,
                        MergeWhen::WhenReady => MergeWhen::Now,
                    };
                }
                AutoMergeChoice::Enabled { .. } => {
                    form.error = Some("auto-merge is already on".to_string())
                }
                AutoMergeChoice::Unavailable => {
                    form.error = Some(
                        "merge when ready needs auto-merge allowed and something to wait for"
                            .to_string(),
                    )
                }
            },
            (MergeStep::Choose, KeyCode::Char('x')) => match auto_merge {
                AutoMergeChoice::Enabled { can_disable: true } => {
                    form.step = MergeStep::ConfirmDisableAutoMerge
                }
                AutoMergeChoice::Enabled { can_disable: false } => {
                    form.error = Some("you cannot turn off auto-merge here".to_string())
                }
                _ => form.error = Some("auto-merge is not on".to_string()),
            },
            (MergeStep::Choose, KeyCode::Enter) => {
                let blockers = merge_blockers(&detail);
                if let Some(blocker) = blockers.iter().find(|blocker| blocker.is_final()) {
                    form.error = Some(blocker.to_string());
                } else if form.when == MergeWhen::WhenReady
                    || !matches!(auto_merge, AutoMergeChoice::Enabled { .. })
                {
                    form.step = MergeStep::ConfirmMerge;
                } else {
                    form.error = Some(
                        "auto-merge is on; turn it off (x) before merging by hand".to_string(),
                    );
                }
            }
            (MergeStep::ConfirmMerge, KeyCode::Enter) => {
                let options = form.options(&head);
                self.start_forge_mutation(ForgeMutation::Merge { number, options });
            }
            (MergeStep::ConfirmDisableAutoMerge, KeyCode::Enter) => {
                self.start_forge_mutation(ForgeMutation::DisableAutoMerge { number });
            }
            _ => {}
        }
        Some(KeyOutcome::Handled)
    }

    /// GitHub answered a merge or an auto-merge change.
    pub(super) fn finish_merge(
        &mut self,
        number: u64,
        result: Result<MutationOutcome, ForgeError>,
    ) {
        match result {
            Ok(outcome) => {
                if matches!(self.pull_requests.modal(), Some(PullRequestModal::Merge(_))) {
                    self.pull_requests.set_modal(None);
                }
                let (message, variant) = merge_outcome_message(number, &outcome);
                self.show_snackbar(message, variant);
                self.reload_after_mutation(true);
            }
            Err(error) => {
                let message = format!("GitHub did not merge #{number}: {error}");
                if let Some(PullRequestModal::Merge(form)) = self.pull_requests.modal_mut() {
                    form.step = MergeStep::Choose;
                    form.error = Some(message.clone());
                }
                self.show_snackbar(message, SnackbarVariant::Error);
            }
        }
    }
}

/// The snackbar for a finished merge, an error when the head branch failed
/// to delete after a successful merge.
pub(in crate::app) fn merge_outcome_message(
    number: u64,
    outcome: &MutationOutcome,
) -> (String, SnackbarVariant) {
    match outcome {
        MutationOutcome::Merged(MergeOutcome::AutoMergeEnabled) => (
            format!("auto-merge enabled on #{number}; GitHub merges it when ready"),
            SnackbarVariant::Info,
        ),
        MutationOutcome::Merged(MergeOutcome::Merged { head_branch }) => {
            let (branch, variant) = match head_branch {
                HeadBranchOutcome::Kept => ("head branch kept".to_string(), SnackbarVariant::Info),
                HeadBranchOutcome::Deleted => {
                    ("head branch deleted".to_string(), SnackbarVariant::Info)
                }
                HeadBranchOutcome::AlreadyDeleted => (
                    "head branch already deleted".to_string(),
                    SnackbarVariant::Info,
                ),
                HeadBranchOutcome::NotDeletable => (
                    "head branch kept (fork or no permission)".to_string(),
                    SnackbarVariant::Info,
                ),
                HeadBranchOutcome::DeleteFailed { message } => (
                    format!("but the head branch could not be deleted: {message}"),
                    SnackbarVariant::Error,
                ),
            };
            (format!("merged #{number} · {branch}"), variant)
        }
        MutationOutcome::AutoMergeDisabled => (
            format!("auto-merge turned off on #{number}"),
            SnackbarVariant::Info,
        ),
        _ => (format!("updated #{number}"), SnackbarVariant::Info),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::pull_request_fixtures as fixtures,
        forge::{AutoMerge, CheckCounts, CheckRollup},
    };

    fn detail() -> PullRequest {
        let mut detail = fixtures::pull_request(18, Vec::new());
        detail.mergeable = Mergeability::Mergeable;
        detail.merge_state = MergeStateStatus::Clean;
        detail
    }

    #[test]
    fn the_form_starts_from_the_repository_settings() {
        let mut detail = detail();
        detail.merge_settings.allowed_methods = vec![MergeMethod::Merge, MergeMethod::Rebase];
        detail.merge_settings.viewer_default_method = MergeMethod::Rebase;
        detail.merge_settings.delete_branch_on_merge = false;
        let mut form = MergeForm::new(&detail);

        assert_eq!(form.method(), MergeMethod::Rebase);
        assert_eq!(form.head_branch(), HeadBranchAction::Keep);
        form.cycle_method(&detail, 1);
        assert_eq!(
            form.method(),
            MergeMethod::Merge,
            "wraps within allowed methods"
        );

        detail.merge_settings.viewer_default_method = MergeMethod::Squash;
        detail.merge_settings.delete_branch_on_merge = true;
        let form = MergeForm::new(&detail);
        assert_eq!(
            form.method(),
            MergeMethod::Merge,
            "a disallowed default falls back to the first allowed method"
        );
        assert_eq!(form.head_branch(), HeadBranchAction::Delete);
    }

    #[test]
    fn options_pin_the_shown_head_and_carry_the_choices() {
        let mut form = MergeForm::new(&detail());
        let head = "a".repeat(40);

        assert_eq!(
            form.options(&head),
            MergeOptions {
                method: MergeMethod::Squash,
                expected_head_oid: head.clone(),
                timing: MergeTiming::Now {
                    head_branch: HeadBranchAction::Delete
                },
                commit_message: None,
            }
        );
        form.when = MergeWhen::WhenReady;
        assert_eq!(form.options(&head).timing, MergeTiming::WhenReady);
    }

    #[test]
    fn blockers_are_described_in_words() {
        let mut detail = detail();
        assert!(merge_blockers(&detail).is_empty());

        detail.merge_state = MergeStateStatus::Blocked;
        detail.summary.review_decision = Some(ReviewDecision::ReviewRequired);
        detail.summary.checks = Some(CheckRollup {
            state: CheckState::Pending,
            counts: CheckCounts {
                pending: 2,
                failure: 1,
                ..CheckCounts::default()
            },
        });
        let blockers = merge_blockers(&detail);
        assert_eq!(
            blockers,
            vec![
                MergeBlocker::ReviewRequired,
                MergeBlocker::ChecksFailing(1),
                MergeBlocker::ChecksPending(2),
            ]
        );
        assert_eq!(blockers[2].to_string(), "2 checks are still running");

        detail.summary.is_draft = true;
        assert!(merge_blockers(&detail)[0].is_final());
        detail.summary.state = PullRequestState::Merged;
        assert_eq!(merge_blockers(&detail), vec![MergeBlocker::Merged]);
    }

    #[test]
    fn auto_merge_is_offered_only_when_allowed_and_waiting() {
        let mut detail = detail();
        detail.merge_settings.auto_merge_allowed = true;
        detail.viewer.can_enable_auto_merge = true;
        assert_eq!(auto_merge_choice(&detail), AutoMergeChoice::Unavailable);

        detail.merge_state = MergeStateStatus::Blocked;
        assert_eq!(auto_merge_choice(&detail), AutoMergeChoice::Offered);

        detail.merge_settings.auto_merge_allowed = false;
        assert_eq!(auto_merge_choice(&detail), AutoMergeChoice::Unavailable);

        detail.auto_merge = Some(AutoMerge {
            method: MergeMethod::Squash,
            enabled_by: Some("Scott-fo".to_string()),
            enabled_at: None,
        });
        detail.viewer.can_disable_auto_merge = true;
        assert_eq!(
            auto_merge_choice(&detail),
            AutoMergeChoice::Enabled { can_disable: true }
        );
    }

    #[test]
    fn a_failed_branch_deletion_is_reported_as_an_error() {
        let (message, variant) = merge_outcome_message(
            18,
            &MutationOutcome::Merged(MergeOutcome::Merged {
                head_branch: HeadBranchOutcome::DeleteFailed {
                    message: "reference is protected".to_string(),
                },
            }),
        );
        assert_eq!(variant, SnackbarVariant::Error);
        assert!(message.contains("merged #18"));
        assert!(message.contains("reference is protected"));
    }
}
