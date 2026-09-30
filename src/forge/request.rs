//! Request construction: GraphQL documents, variables, REST bodies, and `gh`
//! arguments.
//!
//! Everything here is pure so mutation payloads can be verified without
//! sending them. Documents live in `graphql/*.graphql`; each operation is
//! concatenated with exactly the fragments it uses, since GitHub rejects
//! documents with unused fragments.

use serde_json::{Map, Value, json};

use super::{
    error::ForgeError,
    types::{
        DiffSide, DraftReviewComment, MergeMethod, MergeOptions, MergeTiming,
        PullRequestListFilter, RepositoryRef, ReviewEvent, SubmitReview, ThreadId,
    },
};

macro_rules! document {
    ($($file:literal),+ $(,)?) => {
        concat!($(include_str!(concat!("graphql/", $file, ".graphql"))),+)
    };
}

pub(super) mod documents {
    pub const PULL_REQUEST: &str = document!(
        "pull_request",
        "summary_fields",
        "review_request_fields",
        "review_fields",
        "conversation_comment_fields",
        "thread_fields",
        "thread_comment_fields",
        "check_fields",
    );
    pub const SUMMARY: &str = document!("summary", "summary_fields");
    pub const SEARCH: &str = document!("search", "summary_fields");
    pub const REVIEW_THREADS_PAGE: &str = document!(
        "review_threads_page",
        "thread_fields",
        "thread_comment_fields"
    );
    pub const THREAD_COMMENTS_PAGE: &str =
        document!("thread_comments_page", "thread_comment_fields");
    pub const CONVERSATION_PAGE: &str =
        document!("conversation_page", "conversation_comment_fields");
    pub const LATEST_REVIEWS_PAGE: &str = document!("latest_reviews_page", "review_fields");
    pub const REVIEW_REQUESTS_PAGE: &str =
        document!("review_requests_page", "review_request_fields");
    pub const CHECKS_PAGE: &str = document!("checks_page", "check_fields");
    pub const PULL_REQUEST_ID: &str = document!("pull_request_id");
    pub const MERGE_TARGET: &str = document!("merge_target");
    pub const REPOSITORY: &str = document!("repository");

    pub const MERGE: &str = document!("merge");
    pub const ENABLE_AUTO_MERGE: &str = document!("enable_auto_merge");
    pub const DISABLE_AUTO_MERGE: &str = document!("disable_auto_merge");
    pub const CLOSE: &str = document!("close");
    pub const REOPEN: &str = document!("reopen");
    pub const MARK_READY: &str = document!("mark_ready");
    pub const CONVERT_TO_DRAFT: &str = document!("convert_to_draft");
    pub const REPLY_TO_THREAD: &str = document!("reply_to_thread", "thread_comment_fields");
    pub const RESOLVE_THREAD: &str = document!("resolve_thread");
    pub const UNRESOLVE_THREAD: &str = document!("unresolve_thread");
    pub const ADD_COMMENT: &str = document!("add_comment", "conversation_comment_fields");

    /// Every read-only document, for live schema validation.
    #[cfg(test)]
    pub const QUERIES: &[(&str, &str)] = &[
        ("PULL_REQUEST", PULL_REQUEST),
        ("SUMMARY", SUMMARY),
        ("SEARCH", SEARCH),
        ("REVIEW_THREADS_PAGE", REVIEW_THREADS_PAGE),
        ("THREAD_COMMENTS_PAGE", THREAD_COMMENTS_PAGE),
        ("CONVERSATION_PAGE", CONVERSATION_PAGE),
        ("LATEST_REVIEWS_PAGE", LATEST_REVIEWS_PAGE),
        ("REVIEW_REQUESTS_PAGE", REVIEW_REQUESTS_PAGE),
        ("CHECKS_PAGE", CHECKS_PAGE),
        ("PULL_REQUEST_ID", PULL_REQUEST_ID),
        ("MERGE_TARGET", MERGE_TARGET),
        ("REPOSITORY", REPOSITORY),
    ];

    /// Every mutation document, for live schema validation.
    #[cfg(test)]
    pub const MUTATIONS: &[(&str, &str)] = &[
        ("MERGE", MERGE),
        ("ENABLE_AUTO_MERGE", ENABLE_AUTO_MERGE),
        ("DISABLE_AUTO_MERGE", DISABLE_AUTO_MERGE),
        ("CLOSE", CLOSE),
        ("REOPEN", REOPEN),
        ("MARK_READY", MARK_READY),
        ("CONVERT_TO_DRAFT", CONVERT_TO_DRAFT),
        ("REPLY_TO_THREAD", REPLY_TO_THREAD),
        ("RESOLVE_THREAD", RESOLVE_THREAD),
        ("UNRESOLVE_THREAD", UNRESOLVE_THREAD),
        ("ADD_COMMENT", ADD_COMMENT),
    ];
}

/// Most pull requests a list returns; GitHub search pages hold 50.
pub(super) const LIST_LIMIT: usize = 100;

/// One GraphQL operation, sent to `gh api graphql --input -` as JSON so user
/// text never passes through `gh`'s `-f`/`-F` argument parsing.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct GraphqlRequest {
    pub document: &'static str,
    pub variables: Value,
}

impl GraphqlRequest {
    pub fn new(document: &'static str, variables: Value) -> Self {
        Self {
            document,
            variables,
        }
    }

    /// A mutation whose only variable is `$input`.
    fn mutation(document: &'static str, input: Value) -> Self {
        Self::new(document, json!({ "input": input }))
    }

    pub fn body(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "query": self.document,
            "variables": self.variables,
        }))
        .expect("GraphQL request is always serializable")
    }
}

/// Arguments for `gh api`, targeting `repo`'s host.
pub(super) fn api_args(repo: &RepositoryRef, rest: &[&str]) -> Vec<String> {
    let mut args = vec!["api".to_string()];
    if repo.host != "github.com" {
        args.push("--hostname".into());
        args.push(repo.host.clone());
    }
    args.extend(rest.iter().map(|arg| arg.to_string()));
    args
}

pub(super) fn graphql_args(repo: &RepositoryRef) -> Vec<String> {
    api_args(repo, &["graphql", "--input", "-"])
}

pub(super) fn pull_request_variables(repo: &RepositoryRef, number: u64) -> Value {
    json!({ "owner": repo.owner, "name": repo.name, "number": number })
}

pub(super) fn search_query(repo: &RepositoryRef, filter: PullRequestListFilter) -> String {
    let qualifier = match filter {
        PullRequestListFilter::NeedsMyReview => " review-requested:@me",
        PullRequestListFilter::Mine => " author:@me",
        PullRequestListFilter::AllOpen => "",
    };
    format!(
        "repo:{} is:pr is:open{qualifier} sort:updated-desc",
        repo.name_with_owner()
    )
}

fn merge_method_wire(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "MERGE",
        MergeMethod::Squash => "SQUASH",
        MergeMethod::Rebase => "REBASE",
    }
}

fn side_wire(side: DiffSide) -> &'static str {
    match side {
        DiffSide::Left => "LEFT",
        DiffSide::Right => "RIGHT",
    }
}

fn event_wire(event: ReviewEvent) -> &'static str {
    match event {
        ReviewEvent::Comment => "COMMENT",
        ReviewEvent::Approve => "APPROVE",
        ReviewEvent::RequestChanges => "REQUEST_CHANGES",
    }
}

/// `mergePullRequest` or `enablePullRequestAutoMerge`, both pinned to the
/// reviewed head.
pub(super) fn merge_request(pull_request_id: &str, options: &MergeOptions) -> GraphqlRequest {
    let mut input = Map::new();
    input.insert("pullRequestId".into(), json!(pull_request_id));
    input.insert(
        "mergeMethod".into(),
        json!(merge_method_wire(options.method)),
    );
    input.insert("expectedHeadOid".into(), json!(options.expected_head_oid));
    if let Some(message) = &options.commit_message {
        input.insert("commitHeadline".into(), json!(message.headline));
        input.insert("commitBody".into(), json!(message.body));
    }
    let document = match options.timing {
        MergeTiming::Now { .. } => documents::MERGE,
        MergeTiming::WhenReady => documents::ENABLE_AUTO_MERGE,
    };
    GraphqlRequest::mutation(document, Value::Object(input))
}

/// A mutation that takes only the pull request's node id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PullRequestMutation {
    Close,
    Reopen,
    MarkReadyForReview,
    ConvertToDraft,
    DisableAutoMerge,
}

impl PullRequestMutation {
    pub fn request(self, pull_request_id: &str) -> GraphqlRequest {
        let document = match self {
            Self::Close => documents::CLOSE,
            Self::Reopen => documents::REOPEN,
            Self::MarkReadyForReview => documents::MARK_READY,
            Self::ConvertToDraft => documents::CONVERT_TO_DRAFT,
            Self::DisableAutoMerge => documents::DISABLE_AUTO_MERGE,
        };
        GraphqlRequest::mutation(document, json!({ "pullRequestId": pull_request_id }))
    }
}

pub(super) fn reply_request(thread: &ThreadId, body: &str) -> Result<GraphqlRequest, ForgeError> {
    require_text(body, "reply")?;
    Ok(GraphqlRequest::mutation(
        documents::REPLY_TO_THREAD,
        json!({ "pullRequestReviewThreadId": thread.as_str(), "body": body }),
    ))
}

pub(super) fn resolve_request(thread: &ThreadId, resolved: bool) -> GraphqlRequest {
    let document = if resolved {
        documents::RESOLVE_THREAD
    } else {
        documents::UNRESOLVE_THREAD
    };
    GraphqlRequest::mutation(document, json!({ "threadId": thread.as_str() }))
}

pub(super) fn add_comment_request(
    pull_request_id: &str,
    body: &str,
) -> Result<GraphqlRequest, ForgeError> {
    require_text(body, "comment")?;
    Ok(GraphqlRequest::mutation(
        documents::ADD_COMMENT,
        json!({ "subjectId": pull_request_id, "body": body }),
    ))
}

fn require_text(body: &str, what: &str) -> Result<(), ForgeError> {
    if body.trim().is_empty() {
        Err(ForgeError::invalid(format!("the {what} is empty")))
    } else {
        Ok(())
    }
}

/// REST path creating a review: `POST repos/{owner}/{repo}/pulls/{n}/reviews`.
pub(super) fn review_path(repo: &RepositoryRef, number: u64) -> String {
    format!("repos/{}/{}/pulls/{number}/reviews", repo.owner, repo.name)
}

/// The REST body creating and submitting a review in one request.
///
/// Inline comments use `line`/`side` (with `start_line`/`start_side` for
/// ranges), which GitHub resolves against `commit_id`'s diff; the older
/// `position` field is never used. A range whose start equals its end is
/// sent as a single line, since GitHub rejects `start_line == line`.
pub(super) fn review_body(review: &SubmitReview) -> Result<Value, ForgeError> {
    let oid = review.commit_oid.trim();
    if oid.len() < 40 || !oid.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ForgeError::invalid(format!(
            "'{}' is not a full commit id",
            review.commit_oid
        )));
    }
    let has_body = !review.body.trim().is_empty();
    match review.event {
        ReviewEvent::Approve => {}
        ReviewEvent::Comment if !has_body && review.comments.is_empty() => {
            return Err(ForgeError::invalid(
                "a comment review needs a summary or at least one inline comment",
            ));
        }
        ReviewEvent::RequestChanges if !has_body && review.comments.is_empty() => {
            return Err(ForgeError::invalid(
                "requesting changes needs a summary or at least one inline comment",
            ));
        }
        ReviewEvent::Comment | ReviewEvent::RequestChanges => {}
    }

    let comments = review
        .comments
        .iter()
        .map(review_comment_body)
        .collect::<Result<Vec<_>, _>>()?;

    let mut body = Map::new();
    body.insert("commit_id".into(), json!(oid));
    body.insert("event".into(), json!(event_wire(review.event)));
    if has_body {
        body.insert("body".into(), json!(review.body));
    }
    if !comments.is_empty() {
        body.insert("comments".into(), Value::Array(comments));
    }
    Ok(Value::Object(body))
}

fn review_comment_body(comment: &DraftReviewComment) -> Result<Value, ForgeError> {
    if comment.path.is_empty() {
        return Err(ForgeError::invalid("an inline comment has no file path"));
    }
    if comment.body.trim().is_empty() {
        return Err(ForgeError::invalid(format!(
            "an inline comment on {}:{} is empty",
            comment.path, comment.end.line
        )));
    }
    if comment.end.line == 0 {
        return Err(ForgeError::invalid(format!(
            "an inline comment on {} has no line",
            comment.path
        )));
    }
    let mut body = Map::new();
    body.insert("path".into(), json!(comment.path));
    body.insert("body".into(), json!(comment.body));
    body.insert("line".into(), json!(comment.end.line));
    body.insert("side".into(), json!(side_wire(comment.end.side)));
    if let Some(start) = comment.start.filter(|start| *start != comment.end) {
        if start.line == 0 || (start.side == comment.end.side && start.line > comment.end.line) {
            return Err(ForgeError::invalid(format!(
                "an inline comment range on {} starts after it ends",
                comment.path
            )));
        }
        body.insert("start_line".into(), json!(start.line));
        body.insert("start_side".into(), json!(side_wire(start.side)));
    }
    Ok(Value::Object(body))
}

/// REST path of a branch ref, percent-encoding each path segment.
pub(super) fn head_ref_path(repo: &RepositoryRef, branch: &str) -> String {
    let encoded = branch
        .split('/')
        .map(percent_encode_segment)
        .collect::<Vec<_>>()
        .join("/");
    format!(
        "repos/{}/{}/git/refs/heads/{encoded}",
        repo.owner, repo.name
    )
}

fn percent_encode_segment(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::types::{DiffPosition, HeadBranchAction, MergeCommitMessage, MergeTiming};

    const OID: &str = "4696b9d09644c6bd6075b426043fe4b09f2ab48f";

    fn repo() -> RepositoryRef {
        RepositoryRef {
            host: "github.com".into(),
            owner: "Scott-fo".into(),
            name: "vigil".into(),
        }
    }

    fn comment(start: Option<DiffPosition>, end: DiffPosition) -> DraftReviewComment {
        DraftReviewComment {
            path: "src/lib.rs".into(),
            end,
            start,
            body: "nit".into(),
        }
    }

    fn right(line: u32) -> DiffPosition {
        DiffPosition {
            side: DiffSide::Right,
            line,
        }
    }

    #[test]
    fn documents_include_only_their_fragments() {
        assert!(documents::SUMMARY.contains("fragment SummaryFields"));
        assert!(!documents::SUMMARY.contains("fragment ThreadFields"));
        assert!(documents::REPLY_TO_THREAD.contains("fragment ThreadCommentFields"));
        for (name, document) in documents::QUERIES.iter().chain(documents::MUTATIONS) {
            for fragment in document
                .match_indices("fragment ")
                .map(|(index, _)| document[index + 9..].split_whitespace().next().unwrap())
            {
                assert!(
                    document.contains(&format!("...{fragment}")),
                    "{name} defines unused fragment {fragment}"
                );
            }
        }
    }

    #[test]
    fn graphql_body_carries_document_and_variables() {
        let request = GraphqlRequest::new(documents::SUMMARY, pull_request_variables(&repo(), 16));
        let body: Value = serde_json::from_slice(&request.body()).unwrap();
        assert_eq!(body["query"], documents::SUMMARY);
        assert_eq!(
            body["variables"],
            json!({ "owner": "Scott-fo", "name": "vigil", "number": 16 })
        );
    }

    #[test]
    fn api_args_add_hostname_only_for_enterprise() {
        assert_eq!(graphql_args(&repo()), ["api", "graphql", "--input", "-"]);
        let enterprise = RepositoryRef {
            host: "ghe.example.com".into(),
            ..repo()
        };
        assert_eq!(
            graphql_args(&enterprise),
            [
                "api",
                "--hostname",
                "ghe.example.com",
                "graphql",
                "--input",
                "-"
            ]
        );
    }

    #[test]
    fn search_query_per_filter() {
        assert_eq!(
            search_query(&repo(), PullRequestListFilter::NeedsMyReview),
            "repo:Scott-fo/vigil is:pr is:open review-requested:@me sort:updated-desc"
        );
        assert_eq!(
            search_query(&repo(), PullRequestListFilter::Mine),
            "repo:Scott-fo/vigil is:pr is:open author:@me sort:updated-desc"
        );
        assert_eq!(
            search_query(&repo(), PullRequestListFilter::AllOpen),
            "repo:Scott-fo/vigil is:pr is:open sort:updated-desc"
        );
    }

    #[test]
    fn review_body_with_single_and_multi_line_comments() {
        let review = SubmitReview {
            commit_oid: OID.into(),
            event: ReviewEvent::RequestChanges,
            body: "Needs work".into(),
            comments: vec![
                comment(None, right(12)),
                comment(
                    Some(DiffPosition {
                        side: DiffSide::Left,
                        line: 3,
                    }),
                    right(8),
                ),
                // A one-line "range" is sent as a single line.
                comment(Some(right(20)), right(20)),
            ],
        };
        assert_eq!(
            review_body(&review).unwrap(),
            json!({
                "commit_id": OID,
                "event": "REQUEST_CHANGES",
                "body": "Needs work",
                "comments": [
                    { "path": "src/lib.rs", "body": "nit", "line": 12, "side": "RIGHT" },
                    { "path": "src/lib.rs", "body": "nit", "line": 8, "side": "RIGHT",
                      "start_line": 3, "start_side": "LEFT" },
                    { "path": "src/lib.rs", "body": "nit", "line": 20, "side": "RIGHT" },
                ],
            })
        );
    }

    #[test]
    fn approval_without_body_omits_body_and_comments() {
        let review = SubmitReview {
            commit_oid: OID.into(),
            event: ReviewEvent::Approve,
            body: "  ".into(),
            comments: Vec::new(),
        };
        assert_eq!(
            review_body(&review).unwrap(),
            json!({ "commit_id": OID, "event": "APPROVE" })
        );
    }

    #[test]
    fn review_body_rejects_invalid_reviews() {
        let empty_comment = SubmitReview {
            commit_oid: OID.into(),
            event: ReviewEvent::Comment,
            body: String::new(),
            comments: Vec::new(),
        };
        assert!(matches!(
            review_body(&empty_comment),
            Err(ForgeError::InvalidRequest { .. })
        ));

        let short_oid = SubmitReview {
            commit_oid: "4696b9d".into(),
            event: ReviewEvent::Approve,
            body: String::new(),
            comments: Vec::new(),
        };
        assert!(review_body(&short_oid).is_err());

        let backwards = SubmitReview {
            commit_oid: OID.into(),
            event: ReviewEvent::Comment,
            body: String::new(),
            comments: vec![comment(Some(right(9)), right(4))],
        };
        assert!(review_body(&backwards).is_err());
    }

    #[test]
    fn merge_now_pins_head_and_message() {
        let options = MergeOptions {
            method: MergeMethod::Squash,
            expected_head_oid: OID.into(),
            timing: MergeTiming::Now {
                head_branch: HeadBranchAction::Delete,
            },
            commit_message: Some(MergeCommitMessage {
                headline: "Add forge (#17)".into(),
                body: "Body".into(),
            }),
        };
        let request = merge_request("PR_kw", &options);
        assert_eq!(request.document, documents::MERGE);
        assert_eq!(
            request.variables,
            json!({ "input": {
                "pullRequestId": "PR_kw",
                "mergeMethod": "SQUASH",
                "expectedHeadOid": OID,
                "commitHeadline": "Add forge (#17)",
                "commitBody": "Body",
            }})
        );
    }

    #[test]
    fn merge_when_ready_enables_auto_merge() {
        let options = MergeOptions {
            method: MergeMethod::Rebase,
            expected_head_oid: OID.into(),
            timing: MergeTiming::WhenReady,
            commit_message: None,
        };
        let request = merge_request("PR_kw", &options);
        assert_eq!(request.document, documents::ENABLE_AUTO_MERGE);
        assert_eq!(
            request.variables,
            json!({ "input": {
                "pullRequestId": "PR_kw",
                "mergeMethod": "REBASE",
                "expectedHeadOid": OID,
            }})
        );
    }

    #[test]
    fn state_mutations_take_the_node_id() {
        for (mutation, document) in [
            (PullRequestMutation::Close, documents::CLOSE),
            (PullRequestMutation::Reopen, documents::REOPEN),
            (
                PullRequestMutation::MarkReadyForReview,
                documents::MARK_READY,
            ),
            (
                PullRequestMutation::ConvertToDraft,
                documents::CONVERT_TO_DRAFT,
            ),
            (
                PullRequestMutation::DisableAutoMerge,
                documents::DISABLE_AUTO_MERGE,
            ),
        ] {
            let request = mutation.request("PR_kw");
            assert_eq!(request.document, document);
            assert_eq!(
                request.variables,
                json!({ "input": { "pullRequestId": "PR_kw" } })
            );
        }
    }

    #[test]
    fn thread_and_comment_mutations() {
        let thread = ThreadId::new("PRRT_kw");
        assert_eq!(
            reply_request(&thread, "Done").unwrap().variables,
            json!({ "input": { "pullRequestReviewThreadId": "PRRT_kw", "body": "Done" } })
        );
        assert!(reply_request(&thread, " \n").is_err());
        let resolve = resolve_request(&thread, true);
        assert_eq!(resolve.document, documents::RESOLVE_THREAD);
        assert_eq!(
            resolve.variables,
            json!({ "input": { "threadId": "PRRT_kw" } })
        );
        assert_eq!(
            resolve_request(&thread, false).document,
            documents::UNRESOLVE_THREAD
        );
        assert_eq!(
            add_comment_request("PR_kw", "LGTM").unwrap().variables,
            json!({ "input": { "subjectId": "PR_kw", "body": "LGTM" } })
        );
    }

    #[test]
    fn rest_paths_encode_branch_segments() {
        assert_eq!(
            review_path(&repo(), 16),
            "repos/Scott-fo/vigil/pulls/16/reviews"
        );
        assert_eq!(
            head_ref_path(&repo(), "forge/github adapter#2"),
            "repos/Scott-fo/vigil/git/refs/heads/forge/github%20adapter%232"
        );
    }
}
