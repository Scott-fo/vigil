//! Writes to GitHub, and the seam tests use to stand in for them.
//!
//! Every write the review screen makes is a typed [`ForgeMutation`]. The
//! app starts at most one at a time; [`ForgeGateway::Live`] runs it on
//! GitHub in the background and reports a [`MutationOutcome`] through the
//! event loop. Tests switch the gateway to recording, which keeps every
//! mutation, follow-up reload, and lookup before opening in a log instead
//! of spawning `gh`, and deliver results by hand. Nothing else in the app reaches the forge's
//! write methods.

use tokio::task;

use crate::{
    event::Event,
    forge::{ForgeError, GitHub, MergeOptions, MergeOutcome, SubmitReview, ThreadId},
    review::DraftId,
};

use super::{
    super::{App, SnackbarVariant},
    PullRequestEvent,
    actions::PullRequestAction,
};

/// A write to GitHub on behalf of the reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum ForgeMutation {
    /// Submits a review; `drafts` are the local drafts it sends, deleted
    /// once GitHub accepts it.
    SubmitReview {
        number: u64,
        review: SubmitReview,
        drafts: Vec<DraftId>,
    },
    ReplyToThread {
        thread: ThreadId,
        body: String,
    },
    SetThreadResolved {
        thread: ThreadId,
        resolved: bool,
    },
    AddConversationComment {
        number: u64,
        body: String,
    },
    Merge {
        number: u64,
        options: MergeOptions,
    },
    DisableAutoMerge {
        number: u64,
    },
    UpdateState {
        number: u64,
        action: PullRequestAction,
    },
}

/// What a finished [`ForgeMutation`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationOutcome {
    ReviewSubmitted,
    Replied,
    ThreadResolved { resolved: bool },
    Commented,
    Merged(MergeOutcome),
    AutoMergeDisabled,
    StateUpdated(PullRequestAction),
}

impl ForgeMutation {
    async fn run(self, github: GitHub) -> Result<MutationOutcome, ForgeError> {
        match self {
            Self::SubmitReview { number, review, .. } => github
                .submit_review(number, &review)
                .await
                .map(|_| MutationOutcome::ReviewSubmitted),
            Self::ReplyToThread { thread, body } => github
                .reply_to_thread(&thread, &body)
                .await
                .map(|_| MutationOutcome::Replied),
            Self::SetThreadResolved { thread, resolved } => {
                if resolved {
                    github.resolve_thread(&thread).await?;
                } else {
                    github.unresolve_thread(&thread).await?;
                }
                Ok(MutationOutcome::ThreadResolved { resolved })
            }
            Self::AddConversationComment { number, body } => github
                .add_conversation_comment(number, &body)
                .await
                .map(|_| MutationOutcome::Commented),
            Self::Merge { number, options } => github
                .merge_pull_request(number, &options)
                .await
                .map(MutationOutcome::Merged),
            Self::DisableAutoMerge { number } => github
                .disable_auto_merge(number)
                .await
                .map(|_| MutationOutcome::AutoMergeDisabled),
            Self::UpdateState { number, action } => {
                match action {
                    PullRequestAction::Close => github.close_pull_request(number).await?,
                    PullRequestAction::Reopen => github.reopen_pull_request(number).await?,
                    PullRequestAction::MarkReadyForReview => {
                        github.mark_ready_for_review(number).await?
                    }
                    PullRequestAction::ConvertToDraft => github.convert_to_draft(number).await?,
                }
                Ok(MutationOutcome::StateUpdated(action))
            }
        }
    }
}

/// Where forge mutations and their follow-up reloads go.
#[derive(Debug, Default)]
pub(in crate::app) enum ForgeGateway {
    /// Run them on GitHub.
    #[default]
    Live,
    /// Record them; nothing is sent and nothing reloads.
    #[cfg(test)]
    Recording(Vec<ForgeCall>),
}

/// A request the recording gateway saw.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) enum ForgeCall {
    Mutation(ForgeMutation),
    ReloadDetail,
    RefreshCurrentBranch,
    /// A pull request looked up by number before opening.
    LookUp(u64),
}

impl App {
    /// Starts `mutation` unless another is still running. Returns whether
    /// it started; the outcome arrives as
    /// [`PullRequestEvent::MutationFinished`].
    pub(in crate::app) fn start_forge_mutation(&mut self, mutation: ForgeMutation) -> bool {
        if self.pull_requests.mutation_in_flight() {
            self.show_snackbar(
                "a GitHub update is still running".to_string(),
                SnackbarVariant::Info,
            );
            return false;
        }
        match self.pull_requests.gateway_mut() {
            // A test must never write to a real repository, even by accident.
            #[cfg(test)]
            ForgeGateway::Live => panic!(
                "tests must record forge mutations (record_forge_calls_for_test): {mutation:?}"
            ),
            #[cfg(not(test))]
            ForgeGateway::Live => {}
            #[cfg(test)]
            ForgeGateway::Recording(log) => {
                log.push(ForgeCall::Mutation(mutation.clone()));
                self.pull_requests.begin_mutation(mutation);
                return true;
            }
        }
        #[cfg_attr(test, allow(unreachable_code))]
        let Some(github) = self.pull_requests.github().cloned() else {
            self.show_snackbar(
                "not connected to GitHub".to_string(),
                SnackbarVariant::Error,
            );
            return false;
        };
        let request_id = self.pull_requests.begin_mutation(mutation.clone());
        let sender = self.events.sender();
        let handle = task::spawn(async move {
            let result = mutation.run(github).await;
            let _ = sender.send(Event::PullRequest(PullRequestEvent::MutationFinished {
                request_id,
                result,
            }));
        });
        self.pull_requests.attach_mutation(request_id, handle);
        true
    }

    /// Reloads what a successful write changed: the open pull request's
    /// detail, and with `branch_chip` the current branch's footer chip.
    pub(in crate::app) fn reload_after_mutation(&mut self, branch_chip: bool) {
        match self.pull_requests.gateway_mut() {
            ForgeGateway::Live => {}
            #[cfg(test)]
            ForgeGateway::Recording(log) => {
                log.push(ForgeCall::ReloadDetail);
                if branch_chip {
                    log.push(ForgeCall::RefreshCurrentBranch);
                }
                return;
            }
        }
        self.reload_open_pull_request_detail();
        if branch_chip {
            self.refresh_current_branch_pull_request(true);
        }
    }

    /// Records mutations instead of running them, for tests.
    #[cfg(test)]
    pub(in crate::app) fn record_forge_calls_for_test(&mut self) {
        *self.pull_requests.gateway_mut() = ForgeGateway::Recording(Vec::new());
    }

    /// What the recording gateway saw so far, oldest first.
    #[cfg(test)]
    pub(in crate::app) fn recorded_forge_calls(&self) -> Vec<ForgeCall> {
        match self.pull_requests.gateway() {
            ForgeGateway::Recording(log) => log.clone(),
            ForgeGateway::Live => Vec::new(),
        }
    }

    /// Delivers the result of the recorded mutation in flight, as if GitHub
    /// had answered.
    #[cfg(test)]
    pub(in crate::app) fn finish_recorded_mutation(
        &mut self,
        result: Result<MutationOutcome, ForgeError>,
    ) -> bool {
        let request_id = self.pull_requests.mutation_request_id();
        self.handle_mutation_finished(request_id, result)
    }
}
