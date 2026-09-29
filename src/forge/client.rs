//! The [`GitHub`] client: pull request reads and writes over `gh`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use super::{
    error::ForgeError,
    gh::run_gh,
    request::{
        GraphqlRequest, LIST_LIMIT, PullRequestMutation, add_comment_request, api_args, documents,
        graphql_args, head_ref_path, merge_request, pull_request_variables, reply_request,
        resolve_request, review_body, review_path, search_query,
    },
    types::{
        ConversationComment, HeadBranchAction, HeadBranchOutcome, MergeOptions, MergeOutcome,
        MergeTiming, PullRequest, PullRequestList, PullRequestListFilter, PullRequestSummary,
        RepositoryRef, Review, SubmitReview, ThreadComment, ThreadId,
    },
    wire::{
        CollectedPages, Connection, FirstPages, MergeTarget, PageInfo, PartialThread,
        PullRequestData, RepositoryView, RestReview, SearchPage, WireCheck,
        WireConversationComment, WireReview, WireReviewRequest, WireSummary, WireThread,
        WireThreadComment, collect_pages, decode, decode_value, graphql_data, take_at,
    },
};

/// A connection to the GitHub repository behind a local checkout.
///
/// Cheap to clone; holds only the repository root and its resolved
/// `owner/name`. Every method runs one or more `gh` processes in the
/// repository root and returns a point-in-time snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHub {
    repo_root: PathBuf,
    repository: RepositoryRef,
}

impl GitHub {
    /// Resolves the GitHub repository for `repo_root` the way `gh` does:
    /// from the git remotes, preferring `upstream` over `origin`, or the
    /// default set with `gh repo set-default`.
    ///
    /// Fails with [`ForgeError::NotGitHubRepository`] when no remote points
    /// at a GitHub host, which callers should treat as "PR features are
    /// unavailable" rather than as an error to report.
    pub async fn connect(repo_root: impl Into<PathBuf>) -> Result<Self, ForgeError> {
        let repo_root = repo_root.into();
        let args = strings(&["repo", "view", "--json", "nameWithOwner,url"]);
        let stdout = run_gh(&repo_root, &args, None).await?;
        let view: RepositoryView = decode("repository", &stdout)?;
        Ok(Self {
            repo_root,
            repository: repository_ref(&view)?,
        })
    }

    /// A client for a known repository, skipping remote resolution.
    pub fn new(repo_root: impl Into<PathBuf>, repository: RepositoryRef) -> Self {
        Self {
            repo_root: repo_root.into(),
            repository,
        }
    }

    pub fn repository(&self) -> &RepositoryRef {
        &self.repository
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// The pull request for the checked-out branch, found the way
    /// `gh pr view` finds it (including branches pushed to forks). `None`
    /// when the branch has no pull request or HEAD is detached.
    pub async fn current_branch_pull_request(
        &self,
    ) -> Result<Option<PullRequestSummary>, ForgeError> {
        #[derive(Deserialize)]
        struct Number {
            number: u64,
        }
        let args = strings(&["pr", "view", "--json", "number"]);
        match run_gh(&self.repo_root, &args, None).await {
            Ok(stdout) => {
                let Number { number } = decode("current branch pull request", &stdout)?;
                self.load_summary(number).await.map(Some)
            }
            Err(error) if is_no_pull_request(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// One pull request's list row.
    pub async fn load_summary(&self, number: u64) -> Result<PullRequestSummary, ForgeError> {
        let request = GraphqlRequest::new(
            documents::SUMMARY,
            pull_request_variables(&self.repository, number),
        );
        let data = self.query("pull request summary", &request).await?;
        let summary: WireSummary = decode_value(
            "pull request summary",
            take_at("pull request summary", data, "/repository/pullRequest")?,
        )?;
        Ok(summary.into())
    }

    /// Open pull requests matching `filter`, most recently updated first,
    /// capped at 100 (see [`PullRequestList::is_truncated`]).
    ///
    /// Backed by GitHub search, which can lag a few seconds behind writes.
    pub async fn list_pull_requests(
        &self,
        filter: PullRequestListFilter,
    ) -> Result<PullRequestList, ForgeError> {
        let query = search_query(&self.repository, filter);
        let mut pull_requests = Vec::new();
        let mut total_count;
        let mut after: Option<String> = None;
        loop {
            let request =
                GraphqlRequest::new(documents::SEARCH, json!({ "query": query, "after": after }));
            let data = self.query("pull request search", &request).await?;
            let page: SearchPage = decode_value(
                "pull request search",
                take_at("pull request search", data, "/search")?,
            )?;
            total_count = page.issue_count;
            pull_requests.extend(
                page.nodes
                    .into_iter()
                    .filter_map(|node| node.into_summary()),
            );
            if pull_requests.len() >= LIST_LIMIT || !page.page_info.has_next_page {
                break;
            }
            match page.page_info.end_cursor {
                Some(cursor) => after = Some(cursor),
                None => break,
            }
        }
        pull_requests.truncate(LIST_LIMIT);
        Ok(PullRequestList {
            pull_requests,
            total_count,
        })
    }

    /// Everything about one pull request: detail, reviews, conversation,
    /// checks, and every review thread with every comment.
    ///
    /// One GraphQL request covers typical pull requests; each collection
    /// that overflows its first page costs one more request per page.
    pub async fn load_pull_request(&self, number: u64) -> Result<PullRequest, ForgeError> {
        let variables = pull_request_variables(&self.repository, number);
        let request = GraphqlRequest::new(documents::PULL_REQUEST, variables.clone());
        let data: PullRequestData =
            decode_value("pull request", self.query("pull request", &request).await?)?;
        let (shell, first) = data.split(number)?;
        let pages = self
            .collect_pull_request_pages(&variables, &shell.head_oid, first)
            .await?;
        Ok(shell.finish(pages))
    }

    async fn collect_pull_request_pages(
        &self,
        variables: &Value,
        head_oid: &str,
        first: FirstPages,
    ) -> Result<CollectedPages, ForgeError> {
        let review_requests = collect_pages("review requests", first.review_requests, |after| {
            self.page::<WireReviewRequest>(
                "review requests",
                documents::REVIEW_REQUESTS_PAGE,
                with_after(variables, &after),
                "/repository/pullRequest/reviewRequests",
            )
        })
        .await?;
        let latest_reviews = collect_pages("reviews", first.latest_reviews, |after| {
            self.page::<WireReview>(
                "reviews",
                documents::LATEST_REVIEWS_PAGE,
                with_after(variables, &after),
                "/repository/pullRequest/latestReviews",
            )
        })
        .await?;
        let conversation = collect_pages("conversation", first.conversation, |after| {
            self.page::<WireConversationComment>(
                "conversation",
                documents::CONVERSATION_PAGE,
                with_after(variables, &after),
                "/repository/pullRequest/comments",
            )
        })
        .await?;
        let threads = collect_pages("review threads", first.review_threads, |after| {
            self.page::<WireThread>(
                "review threads",
                documents::REVIEW_THREADS_PAGE,
                with_after(variables, &after),
                "/repository/pullRequest/reviewThreads",
            )
        })
        .await?;
        let mut review_threads = Vec::with_capacity(threads.len());
        for thread in threads {
            review_threads.push(self.complete_thread(thread.into()).await?.finish());
        }
        let checks = match first.checks {
            Some(first_checks) => {
                let mut check_variables = variables.clone();
                check_variables["oid"] = json!(head_oid);
                collect_pages("checks", first_checks, |after| {
                    self.page::<WireCheck>(
                        "checks",
                        documents::CHECKS_PAGE,
                        with_after(&check_variables, &after),
                        "/repository/object/statusCheckRollup/contexts",
                    )
                })
                .await?
            }
            None => Vec::new(),
        };
        Ok(CollectedPages {
            review_requests,
            latest_reviews,
            conversation,
            review_threads,
            checks,
        })
    }

    async fn complete_thread(
        &self,
        mut thread: PartialThread,
    ) -> Result<PartialThread, ForgeError> {
        if !thread.comments_page.has_next_page {
            return Ok(thread);
        }
        let remaining = Connection {
            page_info: std::mem::replace(
                &mut thread.comments_page,
                PageInfo {
                    has_next_page: false,
                    end_cursor: None,
                },
            ),
            nodes: Vec::new(),
        };
        let thread_id = thread.id.clone();
        let more = collect_pages("thread comments", remaining, |after| {
            self.page::<WireThreadComment>(
                "thread comments",
                documents::THREAD_COMMENTS_PAGE,
                json!({ "id": thread_id, "after": after }),
                "/node/comments",
            )
        })
        .await?;
        thread.extend_comments(more);
        Ok(thread)
    }

    /// Creates and submits a review with all its inline comments in one
    /// request, pinned to `review.commit_oid`.
    ///
    /// Invalid reviews (empty comments, backwards ranges, a missing summary
    /// where GitHub requires one) fail with [`ForgeError::InvalidRequest`]
    /// before contacting GitHub. GitHub rejects the review while the viewer
    /// has a pending review from the web UI (see
    /// [`Viewer::pending_review`](super::Viewer::pending_review)), when
    /// approving one's own pull request, and when a comment's lines are not
    /// part of the diff at `commit_oid`.
    pub async fn submit_review(
        &self,
        number: u64,
        review: &SubmitReview,
    ) -> Result<Review, ForgeError> {
        let body = review_body(review)?;
        let path = review_path(&self.repository, number);
        let args = api_args(
            &self.repository,
            &["--method", "POST", &path, "--input", "-"],
        );
        let stdin = serde_json::to_vec(&body).expect("review body is serializable");
        let stdout = run_gh(&self.repo_root, &args, Some(&stdin)).await?;
        let review: RestReview = decode("submitted review", &stdout)?;
        Ok(review.into())
    }

    /// Posts a reply in a review thread, visible immediately.
    pub async fn reply_to_thread(
        &self,
        thread: &ThreadId,
        body: &str,
    ) -> Result<ThreadComment, ForgeError> {
        let request = reply_request(thread, body)?;
        let data = self.mutate("thread reply", &request).await?;
        let comment: WireThreadComment = decode_value(
            "thread reply",
            take_at(
                "thread reply",
                data,
                "/addPullRequestReviewThreadReply/comment",
            )?,
        )?;
        Ok(comment.into())
    }

    pub async fn resolve_thread(&self, thread: &ThreadId) -> Result<(), ForgeError> {
        self.mutate("resolve thread", &resolve_request(thread, true))
            .await
            .map(drop)
    }

    pub async fn unresolve_thread(&self, thread: &ThreadId) -> Result<(), ForgeError> {
        self.mutate("unresolve thread", &resolve_request(thread, false))
            .await
            .map(drop)
    }

    /// Posts a comment on the pull request's conversation tab.
    pub async fn add_conversation_comment(
        &self,
        number: u64,
        body: &str,
    ) -> Result<ConversationComment, ForgeError> {
        let id = self.pull_request_id(number).await?;
        let request = add_comment_request(&id, body)?;
        let data = self.mutate("conversation comment", &request).await?;
        let comment: WireConversationComment = decode_value(
            "conversation comment",
            take_at("conversation comment", data, "/addComment/commentEdge/node")?,
        )?;
        Ok(comment.into())
    }

    /// Merges now or enables auto-merge, refusing if the head moved past
    /// `options.expected_head_oid`.
    ///
    /// Uses the API rather than `gh pr merge`, which would also switch and
    /// delete local branches. Deleting the head branch affects only the
    /// branch on GitHub, never local branches, and is skipped for forks. A
    /// failed deletion after a successful merge is reported in the outcome,
    /// not as an error.
    pub async fn merge_pull_request(
        &self,
        number: u64,
        options: &MergeOptions,
    ) -> Result<MergeOutcome, ForgeError> {
        let target = self.merge_target(number).await?;
        self.mutate("merge", &merge_request(&target.id, options))
            .await?;
        match options.timing {
            MergeTiming::WhenReady => Ok(MergeOutcome::AutoMergeEnabled),
            MergeTiming::Now { head_branch } => Ok(MergeOutcome::Merged {
                head_branch: self.settle_head_branch(&target, head_branch).await,
            }),
        }
    }

    async fn settle_head_branch(
        &self,
        target: &MergeTarget,
        action: HeadBranchAction,
    ) -> HeadBranchOutcome {
        if action == HeadBranchAction::Keep {
            return HeadBranchOutcome::Kept;
        }
        if target.is_cross_repository || !target.viewer_can_delete_head_ref {
            return HeadBranchOutcome::NotDeletable;
        }
        let path = head_ref_path(&self.repository, &target.head_ref_name);
        let args = api_args(&self.repository, &["--method", "DELETE", &path]);
        match run_gh(&self.repo_root, &args, None).await {
            Ok(_) => HeadBranchOutcome::Deleted,
            Err(error) => head_branch_delete_failure(error),
        }
    }

    pub async fn disable_auto_merge(&self, number: u64) -> Result<(), ForgeError> {
        self.pull_request_mutation(number, PullRequestMutation::DisableAutoMerge)
            .await
    }

    pub async fn close_pull_request(&self, number: u64) -> Result<(), ForgeError> {
        self.pull_request_mutation(number, PullRequestMutation::Close)
            .await
    }

    pub async fn reopen_pull_request(&self, number: u64) -> Result<(), ForgeError> {
        self.pull_request_mutation(number, PullRequestMutation::Reopen)
            .await
    }

    pub async fn mark_ready_for_review(&self, number: u64) -> Result<(), ForgeError> {
        self.pull_request_mutation(number, PullRequestMutation::MarkReadyForReview)
            .await
    }

    pub async fn convert_to_draft(&self, number: u64) -> Result<(), ForgeError> {
        self.pull_request_mutation(number, PullRequestMutation::ConvertToDraft)
            .await
    }

    async fn pull_request_mutation(
        &self,
        number: u64,
        mutation: PullRequestMutation,
    ) -> Result<(), ForgeError> {
        let id = self.pull_request_id(number).await?;
        self.mutate("pull request update", &mutation.request(&id))
            .await
            .map(drop)
    }

    async fn pull_request_id(&self, number: u64) -> Result<String, ForgeError> {
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        let request = GraphqlRequest::new(
            documents::PULL_REQUEST_ID,
            pull_request_variables(&self.repository, number),
        );
        let data = self.query("pull request id", &request).await?;
        let Id { id } = decode_value(
            "pull request id",
            take_at("pull request id", data, "/repository/pullRequest")?,
        )?;
        Ok(id)
    }

    async fn merge_target(&self, number: u64) -> Result<MergeTarget, ForgeError> {
        let request = GraphqlRequest::new(
            documents::MERGE_TARGET,
            pull_request_variables(&self.repository, number),
        );
        let data = self.query("merge target", &request).await?;
        decode_value(
            "merge target",
            take_at("merge target", data, "/repository/pullRequest")?,
        )
    }

    /// Runs a read-only GraphQL request, retrying once on a gateway timeout.
    async fn query(&self, context: &str, request: &GraphqlRequest) -> Result<Value, ForgeError> {
        match self.graphql(context, request).await {
            Err(error) if error.is_transient() => self.graphql(context, request).await,
            result => result,
        }
    }

    /// Runs a mutation exactly once; a retry could apply it twice.
    async fn mutate(&self, context: &str, request: &GraphqlRequest) -> Result<Value, ForgeError> {
        self.graphql(context, request).await
    }

    async fn graphql(&self, context: &str, request: &GraphqlRequest) -> Result<Value, ForgeError> {
        let args = graphql_args(&self.repository);
        let stdout = run_gh(&self.repo_root, &args, Some(&request.body())).await?;
        graphql_data(context, &stdout)
    }

    async fn page<T: DeserializeOwned>(
        &self,
        context: &'static str,
        document: &'static str,
        variables: Value,
        pointer: &'static str,
    ) -> Result<Connection<T>, ForgeError> {
        let data = self
            .query(context, &GraphqlRequest::new(document, variables))
            .await?;
        decode_value(context, take_at(context, data, pointer)?)
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

fn with_after(variables: &Value, after: &str) -> Value {
    let mut variables = variables.clone();
    variables["after"] = json!(after);
    variables
}

fn repository_ref(view: &RepositoryView) -> Result<RepositoryRef, ForgeError> {
    let malformed = || ForgeError::decode("repository", format!("unexpected repository {view:?}"));
    let (owner, name) = view.name_with_owner.split_once('/').ok_or_else(malformed)?;
    let host = view
        .url
        .split_once("://")
        .map_or(view.url.as_str(), |(_, rest)| rest)
        .split('/')
        .next()
        .filter(|host| !host.is_empty())
        .ok_or_else(malformed)?;
    Ok(RepositoryRef {
        host: host.to_string(),
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// `gh pr view` without an argument fails this way when the branch has no
/// pull request or there is no branch.
fn is_no_pull_request(error: &ForgeError) -> bool {
    let message = match error {
        ForgeError::CommandFailed { stderr, .. } => stderr,
        ForgeError::NotFound { message } => message,
        _ => return false,
    };
    let message = message.to_ascii_lowercase();
    message.contains("no pull requests found")
        || message.contains("could not determine current branch")
        || message.contains("not on any branch")
}

fn head_branch_delete_failure(error: ForgeError) -> HeadBranchOutcome {
    match &error {
        ForgeError::NotFound { .. } => HeadBranchOutcome::AlreadyDeleted,
        ForgeError::CommandFailed { stderr, .. } if stderr.contains("Reference does not exist") => {
            HeadBranchOutcome::AlreadyDeleted
        }
        _ => HeadBranchOutcome::DeleteFailed {
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_ref_from_repo_view() {
        let view: RepositoryView =
            serde_json::from_str(include_str!("fixtures/repo_view.json")).unwrap();
        assert_eq!(
            repository_ref(&view).unwrap(),
            RepositoryRef {
                host: "github.com".into(),
                owner: "Scott-fo".into(),
                name: "vigil".into(),
            }
        );
        let enterprise = RepositoryView {
            name_with_owner: "platform/api".into(),
            url: "https://ghe.example.com/platform/api".into(),
        };
        assert_eq!(repository_ref(&enterprise).unwrap().host, "ghe.example.com");
    }

    #[test]
    fn branch_without_pull_request_is_none_not_an_error() {
        let error = ForgeError::CommandFailed {
            args: strings(&["pr", "view"]),
            stderr: r#"no pull requests found for branch "forge/github-adapter""#.into(),
        };
        assert!(is_no_pull_request(&error));
        assert!(!is_no_pull_request(&ForgeError::GhNotInstalled));
    }

    #[test]
    fn already_deleted_branch_is_not_a_failure() {
        let gone = ForgeError::CommandFailed {
            args: Vec::new(),
            stderr: "Reference does not exist (HTTP 422)".into(),
        };
        assert_eq!(
            head_branch_delete_failure(gone),
            HeadBranchOutcome::AlreadyDeleted
        );
        let protected = ForgeError::CommandFailed {
            args: Vec::new(),
            stderr: "Cannot delete this protected branch".into(),
        };
        assert!(matches!(
            head_branch_delete_failure(protected),
            HeadBranchOutcome::DeleteFailed { .. }
        ));
    }

    /// Live tests: need `gh` logged in and network access. They only read;
    /// run with `cargo test forge -- --ignored`.
    mod live {
        use super::*;

        fn repo_root() -> PathBuf {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        }

        #[tokio::test]
        #[ignore = "talks to github.com through gh"]
        async fn loads_a_pull_request_from_this_repository() {
            let github = GitHub::connect(repo_root()).await.unwrap();
            assert_eq!(github.repository().name_with_owner(), "Scott-fo/vigil");

            let pull_request = github.load_pull_request(16).await.unwrap();
            assert_eq!(pull_request.summary.number, 16);
            assert_eq!(
                pull_request.summary.state,
                crate::forge::PullRequestState::Merged
            );
            assert!(!pull_request.viewer.login.is_empty());

            let summary = github.load_summary(15).await.unwrap();
            assert_eq!(summary.head_ref_name, "git/branch-panel");

            let list = github
                .list_pull_requests(PullRequestListFilter::AllOpen)
                .await
                .unwrap();
            assert!(list.pull_requests.len() as u64 <= list.total_count);
            github.current_branch_pull_request().await.unwrap();

            let missing = github.load_pull_request(99_999).await.unwrap_err();
            assert!(
                matches!(missing, ForgeError::NotFound { .. }),
                "{missing:?}"
            );
        }

        #[tokio::test]
        #[ignore = "talks to github.com through gh"]
        async fn follows_review_thread_pages() {
            // tokio-rs/tokio#2273 has 74 review threads, more than one page.
            let github = GitHub::new(
                repo_root(),
                RepositoryRef {
                    host: "github.com".into(),
                    owner: "tokio-rs".into(),
                    name: "tokio".into(),
                },
            );
            let pull_request = github.load_pull_request(2273).await.unwrap();
            assert_eq!(pull_request.review_threads.len(), 74);
            assert!(
                pull_request
                    .review_threads
                    .iter()
                    .all(|thread| !thread.comments.is_empty())
            );
        }

        /// Validates every document against GitHub's schema without running
        /// any mutation: each document is sent alongside a trivial query and
        /// `operationName` selects that query, so GitHub validates the whole
        /// document but executes only the probe. No `$input` is supplied
        /// either, so a mutation could not execute even if selected.
        #[tokio::test]
        #[ignore = "talks to github.com through gh"]
        async fn documents_match_the_github_schema() {
            let repository = RepositoryRef {
                host: "github.com".into(),
                owner: "Scott-fo".into(),
                name: "vigil".into(),
            };
            for (name, document) in documents::QUERIES.iter().chain(documents::MUTATIONS) {
                let body = serde_json::to_vec(&json!({
                    "query": format!("{document}\nquery VigilSchemaProbe {{ viewer {{ login }} }}\n"),
                    "operationName": "VigilSchemaProbe",
                }))
                .unwrap();
                let result = run_gh(&repo_root(), &graphql_args(&repository), Some(&body)).await;
                assert!(result.is_ok(), "{name} failed validation: {result:?}");
            }
        }
    }
}
