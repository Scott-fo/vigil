//! Decoding tests against trimmed responses captured from github.com.

use std::cell::RefCell;

use serde_json::json;

use super::*;
use crate::forge::types::CheckCounts;

const VIGIL_16: &str = include_str!("../fixtures/pull_request_vigil_16.json");
const RATATUI_2511: &str = include_str!("../fixtures/pull_request_ratatui_2511.json");
const TOKIO_7696: &str = include_str!("../fixtures/pull_request_tokio_7696.json");
const TOKIO_7696_THREADS_PAGE: &str =
    include_str!("../fixtures/review_threads_page_tokio_7696.json");
const SEARCH_VIGIL: &str = include_str!("../fixtures/search_vigil.json");
const SUMMARY_RATATUI_2720: &str = include_str!("../fixtures/summary_ratatui_2720.json");

fn pull_request_data(fixture: &str) -> PullRequestData {
    decode_value(
        "fixture",
        graphql_data("fixture", fixture.as_bytes()).unwrap(),
    )
    .unwrap()
}

async fn no_more<T>(_after: String) -> Result<Connection<T>, ForgeError> {
    unreachable!("fixture has one page")
}

/// Mirrors `GitHub::load_pull_request` with canned follow-up pages.
async fn load_fixture(fixture: &str, thread_pages: &[&str]) -> PullRequest {
    let (shell, first) = pull_request_data(fixture).split(1).unwrap();
    let pages = RefCell::new(thread_pages.iter());
    let threads = collect_pages("review threads", first.review_threads, |_after| {
        let page = pages.borrow_mut().next().copied();
        async move {
            let page = page.expect("unexpected extra page");
            decode_value(
                "page",
                take_at(
                    "page",
                    graphql_data("page", page.as_bytes())?,
                    "/repository/pullRequest/reviewThreads",
                )?,
            )
        }
    })
    .await
    .unwrap();
    assert!(pages.borrow_mut().next().is_none(), "unused thread pages");
    let collected = CollectedPages {
        review_requests: collect_pages("requests", first.review_requests, no_more)
            .await
            .unwrap(),
        latest_reviews: collect_pages("reviews", first.latest_reviews, no_more)
            .await
            .unwrap(),
        conversation: collect_pages("conversation", first.conversation, no_more)
            .await
            .unwrap(),
        review_threads: threads
            .into_iter()
            .map(|thread| PartialThread::from(thread).finish())
            .collect(),
        checks: match first.checks {
            Some(checks) => collect_pages("checks", checks, no_more).await.unwrap(),
            None => Vec::new(),
        },
    };
    shell.finish(collected)
}

#[tokio::test]
async fn merged_pull_request_without_checks_or_reviews() {
    let pull_request = load_fixture(VIGIL_16, &[]).await;
    let summary = &pull_request.summary;
    assert_eq!(summary.number, 16);
    assert_eq!(
        summary.title,
        "Copy diff selection to clipboard on mouse release"
    );
    assert_eq!(summary.author, "Scott-fo");
    assert_eq!(summary.state, PullRequestState::Merged);
    assert_eq!(summary.head_ref_name, "diff/copy-on-release");
    assert_eq!(summary.base_ref_name, "master");
    assert_eq!(summary.head_oid.len(), 40);
    assert_eq!(summary.review_decision, None);
    assert_eq!(summary.checks, None);
    assert!(!summary.is_cross_repository);

    assert!(pull_request.checks.is_empty());
    assert!(pull_request.review_threads.is_empty());
    assert!(pull_request.latest_reviews.is_empty());
    assert_eq!(pull_request.commit_count, 1);
    assert_eq!(pull_request.mergeable, Mergeability::Unknown);
    assert_eq!(pull_request.merge_state, MergeStateStatus::Unknown);
    assert_eq!(pull_request.auto_merge, None);
    assert_eq!(pull_request.viewer.login, "Scott-fo");
    assert!(pull_request.viewer.is_author);
    assert_eq!(pull_request.viewer.pending_review, None);
    assert_eq!(
        pull_request.merge_settings,
        MergeSettings {
            allowed_methods: vec![MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase],
            viewer_default_method: MergeMethod::Merge,
            auto_merge_allowed: false,
            delete_branch_on_merge: false,
        }
    );
}

#[tokio::test]
async fn reviewed_pull_request_with_checks_and_conversation() {
    let pull_request = load_fixture(RATATUI_2511, &[]).await;
    let summary = &pull_request.summary;
    assert!(summary.is_cross_repository);
    assert_eq!(summary.review_decision, Some(ReviewDecision::Approved));
    assert_eq!(
        summary.checks,
        Some(CheckRollup {
            state: CheckState::Success,
            counts: CheckCounts {
                success: 41,
                skipped: 1,
                ..CheckCounts::default()
            },
        })
    );
    assert_eq!(summary.checks.unwrap().counts.total(), 42);

    let reviews: Vec<_> = pull_request
        .latest_reviews
        .iter()
        .map(|review| (review.author.as_str(), review.state))
        .collect();
    assert_eq!(
        reviews,
        [
            ("JayanAXHF", ReviewState::Commented),
            ("orhun", ReviewState::Approved)
        ]
    );
    let conversation: Vec<_> = pull_request
        .conversation
        .iter()
        .map(|comment| comment.author.as_str())
        .collect();
    assert_eq!(conversation, ["codecov", "orhun", "pierluigilenoci"]);

    assert_eq!(
        pull_request.checks[1],
        Check {
            name: "Prevent Merging".into(),
            workflow: Some("Check Pull Requests".into()),
            state: CheckState::Skipped,
            summary: None,
            url: Some(
                "https://github.com/ratatui/ratatui/actions/runs/34061114065/job/101561703334"
                    .into()
            ),
            is_required: false,
        }
    );
    assert!(pull_request.checks[2].is_required);
    let codecov = &pull_request.checks[3];
    assert_eq!(codecov.workflow, None);
    assert_eq!(
        codecov.summary.as_deref(),
        Some("100.0% of diff hit (target 92.7%)")
    );
}

#[tokio::test]
async fn single_line_thread_has_no_range_start() {
    let pull_request = load_fixture(RATATUI_2511, &[]).await;
    let [thread] = pull_request.review_threads.as_slice() else {
        panic!("expected one thread");
    };
    // GitHub reports startLine == line for single-line threads.
    assert_eq!(thread.id, ThreadId::new("PRRT_kwDOI9DLB86Cg7g0"));
    assert_eq!(thread.path, "ratatui-widgets/src/table.rs");
    assert_eq!(thread.subject, ThreadSubject::Line);
    assert_eq!(thread.side, DiffSide::Right);
    assert_eq!(thread.line, Some(275));
    assert_eq!(thread.start, None);
    assert_eq!(thread.original_line, Some(275));
    assert_eq!(thread.original_start_line, None);
    assert!(thread.is_resolved);
    assert_eq!(thread.resolved_by.as_deref(), Some("orhun"));
    assert!(
        thread
            .diff_hunk
            .ends_with("keep visible before and after the selected row")
    );
    let authors: Vec<_> = thread.comments.iter().map(|c| c.author.as_str()).collect();
    assert_eq!(authors, ["JayanAXHF", "pierluigilenoci"]);
    assert!(
        thread
            .comments
            .iter()
            .all(|comment| comment.state == CommentState::Submitted)
    );
}

#[tokio::test]
async fn threads_are_collected_across_pages_in_order() {
    let pull_request = load_fixture(TOKIO_7696, &[TOKIO_7696_THREADS_PAGE]).await;
    let ids: Vec<_> = pull_request
        .review_threads
        .iter()
        .map(|thread| thread.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "PRRT_kwDOBAsbdc5eg0jk",
            "PRRT_kwDOBAsbdc5eg0wV",
            "PRRT_kwDOBAsbdc5e4SPf",
            "PRRT_kwDOBAsbdc5gLmNF",
            "PRRT_kwDOBAsbdc5ifa4E",
            "PRRT_kwDOBAsbdc5ifkcT",
        ]
    );
}

#[tokio::test]
async fn thread_anchoring_for_ranges_and_outdated_threads() {
    let pull_request = load_fixture(TOKIO_7696, &[TOKIO_7696_THREADS_PAGE]).await;
    let threads = &pull_request.review_threads;

    // Outdated, but its range still maps onto the current diff.
    assert!(threads[0].is_outdated);
    assert_eq!(threads[0].line, Some(11));
    assert_eq!(
        threads[0].start,
        Some(DiffPosition {
            side: DiffSide::Right,
            line: 8
        })
    );

    // Outdated range that no longer maps: only the originals remain, even
    // though GitHub still reports a start side.
    assert_eq!(threads[1].line, None);
    assert_eq!(threads[1].start, None);
    assert_eq!(threads[1].original_line, Some(85));
    assert_eq!(threads[1].original_start_line, Some(80));

    assert!(!threads[2].is_resolved);
    assert_eq!(threads[2].resolved_by, None);
    assert_eq!(threads[2].comments.len(), 3);

    // Current multi-line range.
    assert!(!threads[3].is_outdated);
    assert_eq!(threads[3].line, Some(49));
    assert_eq!(
        threads[3].start,
        Some(DiffPosition {
            side: DiffSide::Right,
            line: 44
        })
    );
    assert_eq!(threads[3].original_line, Some(41));
    assert_eq!(threads[3].original_start_line, Some(36));
}

#[tokio::test]
async fn thread_comments_continue_on_later_pages() {
    let data = pull_request_data(RATATUI_2511);
    let (_, first) = data.split(2511).unwrap();
    let wire = first.review_threads.nodes.into_iter().next().unwrap();
    let mut thread = PartialThread::from(wire);
    assert!(!thread.comments_page.has_next_page);
    let later: Connection<WireThreadComment> = serde_json::from_value(json!({
        "pageInfo": { "hasNextPage": false, "endCursor": null },
        "nodes": [{
            "id": "PRRC_later",
            "author": null,
            "body": "Third",
            "createdAt": "2026-09-07T00:00:00Z",
            "url": "https://github.com/ratatui/ratatui/pull/2511#discussion_r3",
            "state": "PENDING",
            "diffHunk": "@@ -1 +1 @@"
        }]
    }))
    .unwrap();
    thread.extend_comments(later.nodes);
    let thread = thread.finish();
    assert_eq!(thread.comments.len(), 3);
    let last = thread.comments.last().unwrap();
    assert_eq!(last.author, "ghost");
    assert_eq!(last.state, CommentState::Pending);
    // The hunk still comes from the first comment.
    assert!(thread.diff_hunk.ends_with("selected row"));
}

#[test]
fn search_results_become_summaries() {
    let page: SearchPage = decode_value(
        "search",
        take_at(
            "search",
            graphql_data("search", SEARCH_VIGIL.as_bytes()).unwrap(),
            "/search",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(page.issue_count, 17);
    let summaries: Vec<_> = page
        .nodes
        .into_iter()
        .filter_map(SearchNode::into_summary)
        .collect();
    let numbers: Vec<_> = summaries.iter().map(|summary| summary.number).collect();
    assert_eq!(numbers, [17, 16, 15]);
}

#[test]
fn search_skips_issues() {
    let node: SearchNode = serde_json::from_value(json!({ "__typename": "Issue" })).unwrap();
    assert!(node.into_summary().is_none());
}

#[test]
fn open_summary_with_approval() {
    let summary: WireSummary = decode_value(
        "summary",
        take_at(
            "summary",
            graphql_data("summary", SUMMARY_RATATUI_2720.as_bytes()).unwrap(),
            "/repository/pullRequest",
        )
        .unwrap(),
    )
    .unwrap();
    let summary = PullRequestSummary::from(summary);
    assert_eq!(summary.state, PullRequestState::Open);
    assert_eq!(summary.review_decision, Some(ReviewDecision::Approved));
    assert_eq!(summary.updated_at, Timestamp::new("2026-09-20T21:47:52Z"));
    assert_eq!(
        (summary.additions, summary.deletions, summary.changed_files),
        (44, 13, 5)
    );
}

#[test]
fn missing_pull_request_is_not_found() {
    let mut data: Value = serde_json::from_str(VIGIL_16).unwrap();
    data["data"]["repository"]["pullRequest"] = Value::Null;
    let data: PullRequestData = decode_value("fixture", data["data"].take()).unwrap();
    assert!(matches!(data.split(16), Err(ForgeError::NotFound { .. })));
}

fn summary_json(overrides: Value) -> Value {
    let mut summary = serde_json::from_str::<Value>(SUMMARY_RATATUI_2720).unwrap()["data"]
        ["repository"]["pullRequest"]
        .take();
    for (key, value) in overrides.as_object().unwrap() {
        summary[key] = value.clone();
    }
    summary
}

#[test]
fn unknown_enum_values_degrade_to_fallbacks() {
    let summary: WireSummary = serde_json::from_value(summary_json(json!({
        "reviewDecision": "SOMETHING_NEW",
        "author": null,
    })))
    .unwrap();
    let summary = PullRequestSummary::from(summary);
    assert_eq!(summary.review_decision, None);
    assert_eq!(summary.author, "ghost");

    let check: WireCheck = serde_json::from_value(json!({
        "__typename": "CheckRun",
        "name": "build",
        "status": "COMPLETED",
        "conclusion": "BRAND_NEW",
        "detailsUrl": null,
        "title": "",
        "isRequired": false,
        "checkSuite": null,
    }))
    .unwrap();
    let check = Check::from(check);
    assert_eq!(check.state, CheckState::Neutral);
    assert_eq!(check.summary, None);

    let state: WireMergeStateStatus = serde_json::from_value(json!("MERGE_QUEUED")).unwrap();
    assert_eq!(MergeStateStatus::from(state), MergeStateStatus::Unknown);
    let draft: WireMergeStateStatus = serde_json::from_value(json!("DRAFT")).unwrap();
    assert_eq!(MergeStateStatus::from(draft), MergeStateStatus::Draft);
    let review: WireReviewState = serde_json::from_value(json!("NEW_STATE")).unwrap();
    assert_eq!(ReviewState::from(review), ReviewState::Commented);
}

#[test]
fn unknown_pull_request_state_fails_decoding() {
    let result = serde_json::from_value::<WireSummary>(summary_json(json!({ "state": "LOCKED" })));
    assert!(result.is_err());
}

#[test]
fn check_run_states_collapse() {
    let run = |status: &str, conclusion: Option<&str>| -> CheckState {
        let check: WireCheck = serde_json::from_value(json!({
            "__typename": "CheckRun",
            "name": "ci",
            "status": status,
            "conclusion": conclusion,
            "detailsUrl": null,
            "title": null,
            "isRequired": true,
            "checkSuite": null,
        }))
        .unwrap();
        Check::from(check).state
    };
    assert_eq!(run("IN_PROGRESS", None), CheckState::Pending);
    assert_eq!(run("QUEUED", None), CheckState::Pending);
    assert_eq!(run("COMPLETED", Some("SUCCESS")), CheckState::Success);
    assert_eq!(run("COMPLETED", Some("TIMED_OUT")), CheckState::Failure);
    assert_eq!(
        run("COMPLETED", Some("ACTION_REQUIRED")),
        CheckState::Failure
    );
    assert_eq!(run("COMPLETED", Some("CANCELLED")), CheckState::Cancelled);
    assert_eq!(run("COMPLETED", Some("STALE")), CheckState::Neutral);

    let status: WireCheck = serde_json::from_value(json!({
        "__typename": "StatusContext",
        "context": "ci/circleci",
        "state": "ERROR",
        "targetUrl": "https://circleci.com/1",
        "description": "Your tests failed",
        "isRequired": false,
    }))
    .unwrap();
    let status = Check::from(status);
    assert_eq!(status.name, "ci/circleci");
    assert_eq!(status.state, CheckState::Failure);
    assert_eq!(status.summary.as_deref(), Some("Your tests failed"));
}

#[test]
fn rollup_counts_merge_check_runs_and_statuses() {
    let summary: WireSummary = serde_json::from_value(summary_json(json!({
        "checkRollup": { "nodes": [{ "commit": { "statusCheckRollup": {
            "state": "PENDING",
            "contexts": {
                "checkRunCountsByState": [
                    { "state": "IN_PROGRESS", "count": 2 },
                    { "state": "QUEUED", "count": 1 },
                    { "state": "FAILURE", "count": 1 },
                    { "state": "CANCELLED", "count": 1 },
                    { "state": "SUCCESS", "count": 3 },
                ],
                "statusContextCountsByState": [
                    { "state": "EXPECTED", "count": 1 },
                    { "state": "SUCCESS", "count": 1 },
                ],
            },
        }}}]},
    })))
    .unwrap();
    let rollup = PullRequestSummary::from(summary).checks.unwrap();
    assert_eq!(rollup.state, CheckState::Pending);
    assert_eq!(
        rollup.counts,
        CheckCounts {
            pending: 4,
            success: 4,
            failure: 1,
            cancelled: 1,
            neutral: 0,
            skipped: 0,
        }
    );
    assert_eq!(rollup.counts.get(CheckState::Pending), 4);
}

#[test]
fn requested_reviewers_cover_users_and_teams() {
    let requests: Vec<WireReviewRequest> = serde_json::from_value(json!([
        { "requestedReviewer": { "__typename": "User", "login": "octocat" } },
        { "requestedReviewer": { "__typename": "Team", "slug": "core", "name": "Core" } },
        { "requestedReviewer": { "__typename": "Bot", "login": "copilot" } },
        { "requestedReviewer": null },
    ]))
    .unwrap();
    let reviewers: Vec<_> = requests
        .into_iter()
        .filter_map(WireReviewRequest::into_reviewer)
        .collect();
    assert_eq!(
        reviewers,
        [
            RequestedReviewer::User {
                login: "octocat".into()
            },
            RequestedReviewer::Team {
                slug: "core".into(),
                name: "Core".into()
            },
            RequestedReviewer::User {
                login: "copilot".into()
            },
        ]
    );
}

#[test]
fn rest_review_response() {
    // Shape of `POST /repos/{owner}/{repo}/pulls/{n}/reviews` responses.
    let review: RestReview = serde_json::from_value(json!({
        "id": 80,
        "node_id": "PRR_kwDOAAABbc4",
        "user": { "login": "Scott-fo", "id": 1 },
        "body": "Looks good",
        "state": "APPROVED",
        "html_url": "https://github.com/Scott-fo/vigil/pull/17#pullrequestreview-80",
        "pull_request_url": "https://api.github.com/repos/Scott-fo/vigil/pulls/17",
        "submitted_at": "2026-09-29T12:00:00Z",
        "commit_id": "ecdd80bb57125d7ba9641ffaa4d7d2c19d3f3091",
        "author_association": "OWNER"
    }))
    .unwrap();
    assert_eq!(
        Review::from(review),
        Review {
            id: "PRR_kwDOAAABbc4".into(),
            author: "Scott-fo".into(),
            state: ReviewState::Approved,
            body: "Looks good".into(),
            submitted_at: Some(Timestamp::new("2026-09-29T12:00:00Z")),
            url: "https://github.com/Scott-fo/vigil/pull/17#pullrequestreview-80".into(),
        }
    );
}

fn page(nodes: &[u32], next: Option<&str>) -> Connection<u32> {
    Connection {
        page_info: PageInfo {
            has_next_page: next.is_some(),
            end_cursor: next.map(str::to_string),
        },
        nodes: nodes.to_vec(),
    }
}

#[tokio::test]
async fn collect_pages_follows_cursors() {
    let requested = RefCell::new(Vec::new());
    let items = collect_pages("numbers", page(&[1, 2], Some("a")), |after| {
        requested.borrow_mut().push(after.clone());
        async move {
            Ok(match after.as_str() {
                "a" => page(&[3, 4], Some("b")),
                "b" => page(&[5], None),
                other => panic!("unexpected cursor {other}"),
            })
        }
    })
    .await
    .unwrap();
    assert_eq!(items, [1, 2, 3, 4, 5]);
    assert_eq!(*requested.borrow(), ["a", "b"]);
}

#[tokio::test]
async fn collect_pages_rejects_broken_cursors() {
    let first = Connection {
        page_info: PageInfo {
            has_next_page: true,
            end_cursor: None,
        },
        nodes: vec![1u32],
    };
    let result = collect_pages("numbers", first, |_| async { Ok(page(&[], None)) }).await;
    assert!(matches!(result, Err(ForgeError::DecodeFailed { .. })));

    let stuck = collect_pages("numbers", page(&[1], Some("a")), |_| async {
        Ok(page(&[2], Some("a")))
    })
    .await;
    assert!(matches!(stuck, Err(ForgeError::DecodeFailed { .. })));
}

#[tokio::test]
async fn collect_pages_propagates_fetch_errors() {
    let result = collect_pages("numbers", page(&[1], Some("a")), |_| async {
        Err::<Connection<u32>, _>(ForgeError::RateLimited {
            message: "slow down".into(),
        })
    })
    .await;
    assert!(matches!(result, Err(ForgeError::RateLimited { .. })));
}
