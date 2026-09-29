//! State-transition tests for reviewing and acting on a pull request: the
//! composer, drafts, submit, thread actions, merge, the actions menu, and
//! Esc back to the list. Every write goes through the recording gateway;
//! nothing here reaches GitHub or the user's review database.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    ComposerTarget, MutationOutcome, PullRequestAction, fixtures,
    gateway::{ForgeCall, ForgeMutation},
    modal::PullRequestModal,
};
use crate::{
    app::{App, DiffTextSelection},
    forge::{
        DiffPosition, DiffSide, ForgeError, HeadBranchAction, HeadBranchOutcome, MergeMethod,
        MergeOutcome, MergeTiming, PendingReview, PullRequest, ReviewEvent, ReviewThread,
        Timestamp,
    },
    git::{DiffSelectionPane, DiffSelectionPoint, WhitespaceMode},
    review::{DraftAnchor, DraftComment, DraftLine},
};

const HEAD: &str = "f076e8b7b9fc5ccdf10f7f2af9ca76d7298e8807";

/// Three added lines: display rows 0, 1, 2 are new lines 1, 2, 3.
const ADDED: &str = "diff --git a/src/app.rs b/src/app.rs\n\
                     --- a/src/app.rs\n\
                     +++ b/src/app.rs\n\
                     @@ -1,0 +1,3 @@\n\
                     +fn one() {}\n\
                     +fn two() {}\n\
                     +fn three() {}\n";

/// One hunk with five context lines above a change, as `diff.context=5`
/// would show, then a second hunk far below: rows 0-1 are outside GitHub's
/// three-line hunks, rows 2-4 inside, row 5 removed, row 6 added, then a
/// two-row gap band, then the second hunk.
const WIDE_CONTEXT: &str = "diff --git a/src/app.rs b/src/app.rs\n\
                            --- a/src/app.rs\n\
                            +++ b/src/app.rs\n\
                            @@ -1,6 +1,6 @@\n \
                            a\n \
                            b\n \
                            c\n \
                            d\n \
                            e\n\
                            -old\n\
                            +new\n\
                            @@ -40,1 +40,2 @@\n \
                            forty\n\
                            +added\n";

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
}

async fn keys(app: &mut App, events: impl IntoIterator<Item = KeyEvent>) {
    for event in events {
        app.handle_key_event(event).await.unwrap();
    }
}

async fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        let code = if ch == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(ch)
        };
        app.handle_key_event(press(code)).await.unwrap();
    }
}

fn review_app(detail: PullRequest) -> App {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-review-action-tests"));
    app.record_forge_calls_for_test();
    app.open_pull_request_for_test(detail, vec![fixtures::file("src/app.rs")]);
    app
}

fn snackbar(app: &App) -> String {
    app.snackbar_notice
        .as_ref()
        .map(|notice| notice.message.clone())
        .unwrap_or_default()
}

fn drafts(app: &App) -> Vec<DraftComment> {
    app.pull_requests
        .open()
        .map(|open| open.drafts().all().to_vec())
        .unwrap_or_default()
}

fn modal(app: &App) -> Option<&PullRequestModal> {
    app.pull_requests.modal()
}

fn mutations(app: &App) -> Vec<ForgeMutation> {
    app.recorded_forge_calls()
        .into_iter()
        .filter_map(|call| match call {
            ForgeCall::Mutation(mutation) => Some(mutation),
            _ => None,
        })
        .collect()
}

fn draft_on(line: u32, head: &str, text: &str) -> DraftComment {
    DraftComment::new(
        "src/app.rs".to_string(),
        DraftAnchor {
            start: None,
            end: DraftLine {
                position: DiffPosition {
                    side: DiffSide::Right,
                    line,
                },
                text: text.to_string(),
            },
        },
        "Consider a guard.".to_string(),
        head.to_string(),
    )
}

#[tokio::test]
async fn c_writes_a_draft_on_the_cursor_line_that_draws_and_counts() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    app.show_pull_request_diff_for_test(ADDED, 1);

    keys(&mut app, [press(KeyCode::Char('c'))]).await;
    let Some(PullRequestModal::Composer(composer)) = modal(&app) else {
        panic!("c opens the composer");
    };
    let ComposerTarget::NewDraft { path, anchor } = composer.target() else {
        panic!("a new draft");
    };
    assert_eq!(path, "src/app.rs");
    assert_eq!(
        anchor.end.position,
        DiffPosition {
            side: DiffSide::Right,
            line: 2
        }
    );

    type_text(&mut app, "First line\nsecond line").await;
    keys(&mut app, [ctrl('s')]).await;

    assert!(modal(&app).is_none());
    let saved = drafts(&app);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].body, "First line\nsecond line");
    assert_eq!(saved[0].head_oid, HEAD, "pinned to the reviewed head");
    assert_eq!(saved[0].anchor.end.text, "fn two() {}");
    assert_eq!(
        app.unresolved_thread_count("src/app.rs"),
        1,
        "sidebar marker"
    );
    assert_eq!(app.pending_draft_count(), 1);
    assert!(
        mutations(&app).is_empty(),
        "drafts stay local until a review is submitted"
    );
}

#[tokio::test]
async fn a_drag_selection_comments_on_the_whole_range_and_can_suggest() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    app.show_pull_request_diff_for_test(ADDED, 2);
    let point = |display_index| DiffSelectionPoint {
        display_index,
        pane: DiffSelectionPane::Unified,
        column: 0,
    };
    app.diff_text_selection = Some(DiffTextSelection {
        anchor: point(2),
        head: point(0),
    });

    keys(&mut app, [press(KeyCode::Char('c')), ctrl('g')]).await;

    let Some(PullRequestModal::Composer(composer)) = modal(&app) else {
        panic!("c opens the composer");
    };
    let ComposerTarget::NewDraft { anchor, .. } = composer.target() else {
        panic!("a new draft");
    };
    assert_eq!(anchor.start_line(), Some(1));
    assert_eq!(anchor.end.position.line, 3);
    assert_eq!(
        composer.text().text(),
        "```suggestion\nfn one() {}\nfn two() {}\nfn three() {}\n```\n"
    );
}

#[tokio::test]
async fn c_refuses_lines_outside_githubs_hunks() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));

    for (row, allowed) in [
        (0, false),
        (1, false),
        (2, true),
        (5, true),
        (6, true),
        (7, false),
    ] {
        app.show_pull_request_diff_for_test(WIDE_CONTEXT, row);
        keys(&mut app, [press(KeyCode::Char('c'))]).await;
        assert_eq!(
            modal(&app).is_some(),
            allowed,
            "row {row}: {}",
            snackbar(&app)
        );
        app.pull_requests.set_modal(None);
    }

    app.show_pull_request_diff_for_test(WIDE_CONTEXT, 0);
    keys(&mut app, [press(KeyCode::Char('c'))]).await;
    assert!(snackbar(&app).contains("within 3 lines of a change"));
    app.show_pull_request_diff_for_test(WIDE_CONTEXT, 7);
    keys(&mut app, [press(KeyCode::Char('c'))]).await;
    assert!(
        snackbar(&app).contains("not expanded context"),
        "the gap band"
    );
}

#[tokio::test]
async fn c_is_refused_while_whitespace_changes_are_hidden() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    app.show_pull_request_diff_for_test(ADDED, 0);
    app.diff_whitespace_mode = WhitespaceMode::Ignore;

    keys(&mut app, [press(KeyCode::Char('c'))]).await;

    assert!(modal(&app).is_none());
    assert!(snackbar(&app).contains("press W to show whitespace changes"));
}

#[tokio::test]
async fn esc_asks_before_discarding_a_written_comment() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    app.show_pull_request_diff_for_test(ADDED, 0);
    keys(&mut app, [press(KeyCode::Char('c'))]).await;
    type_text(&mut app, "keep me").await;

    keys(&mut app, [press(KeyCode::Esc)]).await;
    assert!(modal(&app).is_some(), "the first esc only asks");
    keys(&mut app, [press(KeyCode::Esc)]).await;
    assert!(modal(&app).is_none());
    assert!(drafts(&app).is_empty());

    keys(&mut app, [press(KeyCode::Char('c')), press(KeyCode::Esc)]).await;
    assert!(modal(&app).is_none(), "an empty composer closes at once");
}

#[tokio::test]
async fn the_overview_c_and_capital_c_comment_on_the_conversation() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    assert!(app.pull_request_overview_visible());

    keys(&mut app, [press(KeyCode::Char('c'))]).await;
    assert!(matches!(
        modal(&app),
        Some(PullRequestModal::Composer(composer))
            if *composer.target() == ComposerTarget::Conversation
    ));
    type_text(&mut app, "Thanks!").await;
    keys(&mut app, [ctrl('s')]).await;

    assert_eq!(
        mutations(&app),
        vec![ForgeMutation::AddConversationComment {
            number: 18,
            body: "Thanks!".to_string()
        }]
    );
    assert!(modal(&app).is_some(), "stays open while posting");
    keys(&mut app, [press(KeyCode::Char('x'))]).await;
    assert!(
        app.finish_recorded_mutation(Ok(MutationOutcome::Commented)),
        "the result is accepted"
    );
    assert!(modal(&app).is_none());
    assert!(
        app.recorded_forge_calls()
            .contains(&ForgeCall::ReloadDetail)
    );
}

#[tokio::test]
async fn a_failed_post_keeps_the_text_and_shows_githubs_error() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    keys(&mut app, [press(KeyCode::Char('C'))]).await;
    type_text(&mut app, "Hello").await;
    keys(&mut app, [ctrl('s')]).await;

    app.finish_recorded_mutation(Err(ForgeError::NotFound {
        message: "Could not resolve to a node".to_string(),
    }));

    let Some(PullRequestModal::Composer(composer)) = modal(&app) else {
        panic!("the composer stays open");
    };
    assert_eq!(composer.text().text(), "Hello");
    assert!(composer.error().unwrap().contains("Could not resolve"));
}

#[tokio::test]
async fn drafts_can_be_edited_and_deleted_from_the_drafts_list() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    app.show_pull_request_diff_for_test(ADDED, 0);
    let draft = draft_on(1, HEAD, "fn one() {}");
    app.save_draft(draft.clone());

    keys(&mut app, [press(KeyCode::Char('D')), press(KeyCode::Enter)]).await;
    let Some(PullRequestModal::Composer(composer)) = modal(&app) else {
        panic!("enter edits the draft");
    };
    assert_eq!(composer.text().text(), "Consider a guard.");
    type_text(&mut app, " Please.").await;
    keys(&mut app, [ctrl('s')]).await;
    assert_eq!(drafts(&app)[0].body, "Consider a guard. Please.");
    assert_eq!(drafts(&app)[0].id, draft.id, "edited in place");

    keys(
        &mut app,
        [press(KeyCode::Char('D')), press(KeyCode::Char('d'))],
    )
    .await;
    assert_eq!(drafts(&app).len(), 1, "the first d only asks");
    keys(&mut app, [press(KeyCode::Char('d'))]).await;
    assert!(drafts(&app).is_empty());
    assert!(modal(&app).is_none());
    assert_eq!(app.unresolved_thread_count("src/app.rs"), 0);
}

#[tokio::test]
async fn submitting_needs_a_body_or_drafts_except_for_approvals() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));

    keys(&mut app, [press(KeyCode::Char('S')), ctrl('s')]).await;
    let Some(PullRequestModal::Submit(form)) = modal(&app) else {
        panic!("S opens the submit form");
    };
    assert!(
        form.error()
            .unwrap()
            .contains("needs a summary or a draft comment")
    );
    assert!(mutations(&app).is_empty());

    keys(&mut app, [press(KeyCode::Tab), ctrl('s')]).await;
    assert!(matches!(
        mutations(&app).as_slice(),
        [ForgeMutation::SubmitReview { review, .. }]
            if review.event == ReviewEvent::Approve && review.comments.is_empty()
    ));
}

#[tokio::test]
async fn submitting_sends_the_drafts_pinned_to_their_head_and_clears_them() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    let kept = draft_on(1, HEAD, "fn one() {}");
    let stale = draft_on(9, "0000000000000000000000000000000000000000", "gone");
    app.save_draft(kept.clone());
    app.save_draft(stale.clone());

    keys(&mut app, [press(KeyCode::Char('S'))]).await;
    type_text(&mut app, "Two nits.").await;
    keys(&mut app, [ctrl('s')]).await;

    let recorded = mutations(&app);

    let [
        ForgeMutation::SubmitReview {
            number,
            review,
            drafts: sent,
        },
    ] = recorded.as_slice()
    else {
        panic!("one review submitted: {:?}", mutations(&app));
    };
    assert_eq!(*number, 18);
    assert_eq!(review.commit_oid, HEAD);
    assert_eq!(review.body, "Two nits.");
    assert_eq!(review.comments, vec![kept.to_review_comment()]);
    assert_eq!(sent, &vec![kept.id.clone()], "drafts left behind stay out");

    app.finish_recorded_mutation(Ok(MutationOutcome::ReviewSubmitted));

    assert_eq!(
        drafts(&app),
        vec![stale],
        "only the sent drafts are cleared"
    );
    assert!(modal(&app).is_none());
    assert!(snackbar(&app).contains("review submitted on #18 with 1 comment"));
    assert!(
        app.recorded_forge_calls()
            .contains(&ForgeCall::ReloadDetail)
    );
}

#[tokio::test]
async fn a_rejected_review_keeps_the_drafts_and_shows_the_error() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));
    app.save_draft(draft_on(1, HEAD, "fn one() {}"));
    keys(&mut app, [press(KeyCode::Char('S')), ctrl('s')]).await;

    app.finish_recorded_mutation(Err(ForgeError::CommandFailed {
        args: Vec::new(),
        stderr: "Unprocessable Entity: Line could not be resolved".to_string(),
    }));

    assert_eq!(drafts(&app).len(), 1);
    let Some(PullRequestModal::Submit(form)) = modal(&app) else {
        panic!("the form stays open");
    };
    assert!(form.error().unwrap().contains("Line could not be resolved"));
}

#[tokio::test]
async fn authors_can_only_comment_and_pending_reviews_and_new_commits_are_warned() {
    let mut detail = fixtures::pull_request(18, Vec::new());
    detail.viewer.is_author = true;
    detail.viewer.pending_review = Some(PendingReview {
        id: "PRR_1".to_string(),
        created_at: Timestamp::new("2026-09-29T10:00:00Z"),
        body: String::new(),
        comment_count: 2,
    });
    let mut app = review_app(detail);
    let newer = "1111111111111111111111111111111111111111";
    app.pull_requests
        .open_mut()
        .unwrap()
        .set_newer_head_for_test(newer);

    keys(&mut app, [press(KeyCode::Char('S')), press(KeyCode::Tab)]).await;
    let Some(PullRequestModal::Submit(form)) = modal(&app) else {
        panic!("S opens the submit form");
    };
    assert_eq!(
        form.event(),
        ReviewEvent::Comment,
        "tab skips approve and request changes"
    );

    let warnings = app.submit_warnings();
    assert!(warnings.contains(&super::SubmitWarning::PendingReviewOnGitHub { comment_count: 2 }));
    assert!(warnings.contains(&super::SubmitWarning::NewCommits {
        reviewed: HEAD.to_string(),
        latest: newer.to_string(),
    }));
}

fn thread_on_two(resolved: bool) -> ReviewThread {
    let mut thread = fixtures::thread("T1", "src/app.rs", DiffSide::Right, Some(2));
    thread.is_resolved = resolved;
    thread.viewer_can_resolve = !resolved;
    thread.viewer_can_unresolve = resolved;
    thread
}

#[tokio::test]
async fn r_replies_and_t_resolves_the_thread_on_the_cursor_line() {
    let mut app = review_app(fixtures::pull_request(18, vec![thread_on_two(false)]));
    app.show_pull_request_diff_for_test(ADDED, 0);

    keys(&mut app, [press(KeyCode::Char('R'))]).await;
    assert!(modal(&app).is_none(), "no thread on line 1");
    assert!(snackbar(&app).contains("no review thread on this line"));

    app.selected_diff_line_index = 1;
    keys(&mut app, [press(KeyCode::Char('R'))]).await;
    type_text(&mut app, "Done.").await;
    keys(&mut app, [ctrl('s')]).await;
    assert_eq!(
        mutations(&app),
        vec![ForgeMutation::ReplyToThread {
            thread: crate::forge::ThreadId::new("T1"),
            body: "Done.".to_string()
        }]
    );
    app.finish_recorded_mutation(Ok(MutationOutcome::Replied));
    assert!(modal(&app).is_none());

    keys(&mut app, [press(KeyCode::Char('T'))]).await;
    assert_eq!(
        mutations(&app).last(),
        Some(&ForgeMutation::SetThreadResolved {
            thread: crate::forge::ThreadId::new("T1"),
            resolved: true
        })
    );
}

#[tokio::test]
async fn only_one_write_runs_at_a_time() {
    let mut app = review_app(fixtures::pull_request(18, vec![thread_on_two(false)]));
    app.show_pull_request_diff_for_test(ADDED, 1);

    keys(
        &mut app,
        [press(KeyCode::Char('T')), press(KeyCode::Char('T'))],
    )
    .await;

    assert_eq!(mutations(&app).len(), 1);
    assert!(snackbar(&app).contains("still running"));
}

fn mergeable_detail() -> PullRequest {
    let mut detail = fixtures::pull_request(18, Vec::new());
    detail.mergeable = crate::forge::Mergeability::Mergeable;
    detail.merge_state = crate::forge::MergeStateStatus::Clean;
    detail
}

#[tokio::test]
async fn merging_takes_a_confirmation_and_pins_the_shown_head() {
    let mut app = review_app(mergeable_detail());

    keys(&mut app, [press(KeyCode::Char('M')), press(KeyCode::Enter)]).await;
    assert!(
        mutations(&app).is_empty(),
        "the first enter asks to confirm"
    );
    keys(&mut app, [press(KeyCode::Enter)]).await;

    let recorded = mutations(&app);
    let [ForgeMutation::Merge { number, options }] = recorded.as_slice() else {
        panic!("one merge: {:?}", mutations(&app));
    };
    assert_eq!(*number, 18);
    assert_eq!(options.method, MergeMethod::Squash);
    assert_eq!(options.expected_head_oid, HEAD);
    assert_eq!(
        options.timing,
        MergeTiming::Now {
            head_branch: HeadBranchAction::Delete
        }
    );

    app.finish_recorded_mutation(Ok(MutationOutcome::Merged(MergeOutcome::Merged {
        head_branch: HeadBranchOutcome::DeleteFailed {
            message: "protected".to_string(),
        },
    })));
    assert!(modal(&app).is_none());
    assert!(snackbar(&app).contains("merged #18"));
    assert!(snackbar(&app).contains("could not be deleted: protected"));
    let calls = app.recorded_forge_calls();
    assert!(calls.contains(&ForgeCall::ReloadDetail));
    assert!(calls.contains(&ForgeCall::RefreshCurrentBranch));
}

#[tokio::test]
async fn merge_options_follow_the_form() {
    let mut detail = mergeable_detail();
    detail.merge_settings.allowed_methods = vec![MergeMethod::Merge, MergeMethod::Squash];
    detail.merge_settings.delete_branch_on_merge = false;
    let mut app = review_app(detail);

    keys(
        &mut app,
        [
            press(KeyCode::Char('M')),
            press(KeyCode::Left),
            press(KeyCode::Char('d')),
            press(KeyCode::Enter),
            press(KeyCode::Enter),
        ],
    )
    .await;

    let recorded = mutations(&app);
    let [ForgeMutation::Merge { options, .. }] = recorded.as_slice() else {
        panic!("one merge");
    };
    assert_eq!(options.method, MergeMethod::Merge);
    assert_eq!(
        options.timing,
        MergeTiming::Now {
            head_branch: HeadBranchAction::Delete
        }
    );
}

#[tokio::test]
async fn closed_pull_requests_cannot_merge() {
    let mut detail = mergeable_detail();
    detail.summary.state = crate::forge::PullRequestState::Closed;
    let mut app = review_app(detail);

    keys(
        &mut app,
        [
            press(KeyCode::Char('M')),
            press(KeyCode::Enter),
            press(KeyCode::Enter),
        ],
    )
    .await;

    assert!(mutations(&app).is_empty());
    let Some(PullRequestModal::Merge(form)) = modal(&app) else {
        panic!("the form stays open");
    };
    assert_eq!(form.error(), Some("the pull request is closed"));
}

#[tokio::test]
async fn the_actions_menu_lists_what_the_viewer_may_do_and_confirms_closing() {
    let mut detail = fixtures::pull_request(18, Vec::new());
    detail.viewer.can_update = true;
    detail.viewer.can_close = true;
    let mut app = review_app(detail);

    keys(&mut app, [press(KeyCode::Char('A'))]).await;
    let Some(PullRequestModal::Actions(menu)) = modal(&app) else {
        panic!("A opens the actions menu");
    };
    assert_eq!(
        menu.actions(),
        [PullRequestAction::ConvertToDraft, PullRequestAction::Close]
    );

    keys(&mut app, [press(KeyCode::Char('j')), press(KeyCode::Enter)]).await;
    assert!(mutations(&app).is_empty(), "closing asks first");
    keys(&mut app, [press(KeyCode::Enter)]).await;
    assert_eq!(
        mutations(&app),
        vec![ForgeMutation::UpdateState {
            number: 18,
            action: PullRequestAction::Close
        }]
    );
    app.finish_recorded_mutation(Ok(MutationOutcome::StateUpdated(PullRequestAction::Close)));
    assert!(snackbar(&app).contains("closed #18"));
    assert!(
        app.recorded_forge_calls()
            .contains(&ForgeCall::RefreshCurrentBranch)
    );
}

#[tokio::test]
async fn the_actions_menu_stays_shut_without_capabilities() {
    let mut app = review_app(fixtures::pull_request(18, Vec::new()));

    keys(&mut app, [press(KeyCode::Char('A'))]).await;

    assert!(modal(&app).is_none());
    assert!(snackbar(&app).contains("no state changes"));
}

#[test]
fn drafts_follow_new_commits_once_the_new_diff_loads() {
    let mut app = App::new_for_benchmarks(PathBuf::from("/tmp/vigil-review-action-tests"));
    let old_head = "2222222222222222222222222222222222222222";
    let mut detail = fixtures::pull_request(18, Vec::new());
    detail.summary.head_oid = old_head.to_string();
    app.open_pull_request_for_test(detail, vec![fixtures::file("src/app.rs")]);
    let moves = draft_on(1, old_head, "fn two() {}");
    let lost = draft_on(3, old_head, "fn removed() {}");
    app.save_draft(moves.clone());
    app.save_draft(lost.clone());

    let mut reloaded = fixtures::pull_request(18, Vec::new());
    reloaded.summary.head_oid = HEAD.to_string();
    app.open_pull_request_for_test(reloaded, vec![fixtures::file("src/app.rs")]);
    assert_eq!(app.pull_requests.open().unwrap().reviewed_head(), HEAD);
    assert_eq!(app.pending_draft_count(), 0, "nothing re-anchored yet");

    app.review_diff_snapshot = Some(std::sync::Arc::new(
        crate::git::ReviewDiffSnapshot::from_diff_text(ADDED, None).unwrap(),
    ));
    app.reanchor_drafts();

    let drafts = drafts(&app);
    let moved = drafts.iter().find(|draft| draft.id == moves.id).unwrap();
    assert_eq!(moved.head_oid, HEAD);
    assert_eq!(moved.anchor.end.position.line, 2, "followed its text");
    let stayed = drafts.iter().find(|draft| draft.id == lost.id).unwrap();
    assert_eq!(stayed.head_oid, old_head, "needs attention");
    assert_eq!(app.pending_draft_count(), 1);
    assert_eq!(
        app.unresolved_unplaced_thread_count(),
        1,
        "the overview row flags it"
    );
}
