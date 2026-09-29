//! State-transition tests for reviewing a pull request: the composer and
//! drafts. Nothing here reaches GitHub or the user's review database.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{ComposerTarget, fixtures, modal::PullRequestModal};
use crate::{
    app::{App, DiffTextSelection},
    forge::{DiffPosition, DiffSide, PullRequest},
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
