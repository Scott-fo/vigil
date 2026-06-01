use std::{path::PathBuf, sync::Arc};

use crate::event::{DiffPrefetchedEvent, Event};
use crate::review::{
    ReviewFinding, ReviewFindingState, ReviewReport, ReviewSeverity, ReviewSide, ReviewSummary,
    ReviewVerdict,
};

use super::*;

fn build_test_app() -> App {
    App::new_for_benchmarks(PathBuf::from("/tmp/vigil-app-tests"))
}

fn build_cache_key(index: usize) -> DiffCacheKey {
    DiffCacheKey {
        review_scope: "working-tree".to_string(),
        file_path: format!("src/file-{index}.rs"),
        file_status: "M ".to_string(),
    }
}

fn build_diff_view(line_count: usize) -> DiffView {
    let mut diff = format!(
        "diff --git a/src/app.rs b/src/app.rs\n\
--- a/src/app.rs\n\
+++ b/src/app.rs\n\
@@ -1,0 +1,{} @@\n",
        line_count
    );
    for index in 0..line_count {
        diff.push_str(&format!("+fn line_{index}() {{}}\n"));
    }
    git::build_diff_view_from_diff_text(&diff, Some("rust"))
}

fn build_review_snapshot() -> git::ReviewDiffSnapshot {
    git::ReviewDiffSnapshot::from_diff_text(
        concat!(
            "diff --git a/src/file-0.rs b/src/file-0.rs\n",
            "--- a/src/file-0.rs\n",
            "+++ b/src/file-0.rs\n",
            "@@ -1,1 +1,2 @@\n",
            " fn existing() {}\n",
            "+fn from_snapshot() {}\n",
        ),
        Some("test-snapshot"),
    )
    .expect("snapshot fixture should parse")
}

#[tokio::test]
async fn registry_ready_does_not_restart_inflight_diff_load() {
    let mut app = build_test_app();
    app.diff_request_id = 7;
    app.pending_diff_cache_key = Some(build_cache_key(0));
    app.diff_load_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    let registry =
        git::HighlightRegistry::new_for_filetypes([]).expect("empty registry should initialize");
    app.handle_highlight_registry_ready(Ok(registry.into()));

    assert_eq!(app.diff_request_id, 7);
    assert!(app.diff_load_task.is_some());

    app.cancel_inflight_diff_load();
}

#[test]
fn prefetched_complete_highlight_is_cached_as_complete() {
    let mut app = build_test_app();
    let key = build_cache_key(1);
    let plain = build_diff_view(3);
    let highlighted = build_diff_view(3);

    app.handle_diff_prefetched(DiffPrefetchedEvent {
        generation: app.diff_cache_generation,
        key: key.clone(),
        plain,
        highlighted: Some(highlighted),
        highlight_complete: true,
    });

    let (_, complete) = app
        .diff_view_cache
        .get_highlighted(&key)
        .expect("prefetched highlighted view should be cached");
    assert!(complete);
}

#[tokio::test]
async fn prefetched_current_diff_replaces_loading_view() {
    let mut app = build_test_app();
    let key = build_cache_key(2);
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: key.file_path.clone(),
        label: "file-2.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.pending_diff_cache_key = Some(key.clone());
    app.diff_view = DiffView::empty("Loading diff...");
    app.diff_load_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    let changed = app.handle_diff_prefetched(DiffPrefetchedEvent {
        generation: app.diff_cache_generation,
        key,
        plain: build_diff_view(3),
        highlighted: None,
        highlight_complete: false,
    });

    assert!(changed);
    assert!(app.diff_load_task.is_none());
    assert!(app.diff_view.note.is_none());
}

#[test]
fn selected_diff_load_uses_review_snapshot_without_task() {
    let mut app = build_test_app();
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_snapshot = Some(Arc::new(
        build_review_snapshot().with_generation(app.diff_cache_generation),
    ));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_none());
    assert!(app.diff_view.note.is_none());
    assert_eq!(app.diff_view.estimated_display_line_count(), 2);
}

#[tokio::test]
async fn diff_search_index_builds_from_review_snapshot() {
    let mut app = build_test_app();
    app.review_diff_snapshot = Some(Arc::new(
        build_review_snapshot().with_generation(app.diff_cache_generation),
    ));

    app.queue_diff_search_index_load();
    let request_id = app.diff_search_index_request_id;
    let event = app
        .events
        .next()
        .await
        .expect("search index event should be emitted");

    let Event::DiffSearchIndexLoaded {
        request_id: event_request_id,
        result,
    } = event
    else {
        panic!("expected diff search index event");
    };
    assert_eq!(event_request_id, request_id);
    let index = result.expect("snapshot should produce a search index");
    assert_eq!(index.file_count(), 1);
    assert_eq!(index.line_count(), 2);
}

#[tokio::test]
async fn diff_search_modal_waits_for_inflight_review_snapshot() {
    let mut app = build_test_app();
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.open_diff_search_modal();

    assert!(app.diff_search_modal_open);
    assert!(app.diff_search_loading);
    assert!(app.diff_search_load_task.is_none());

    app.cancel_inflight_review_diff_snapshot();
}

#[test]
fn diff_view_cache_touches_recent_entries_before_trimming() {
    let mut cache = DiffViewCache::default();

    for index in 0..DIFF_CACHE_CAPACITY {
        cache.insert_plain(
            build_cache_key(index),
            DiffView::empty(format!("plain-{index}")),
        );
    }

    let touched_key = build_cache_key(0);
    let evicted_key = build_cache_key(1);
    assert!(cache.get_plain(&touched_key).is_some());

    cache.insert_plain(
        build_cache_key(DIFF_CACHE_CAPACITY),
        DiffView::empty("overflow"),
    );

    assert!(cache.get_plain(&touched_key).is_some());
    assert!(cache.get_plain(&evicted_key).is_none());
}

#[test]
fn prepare_diff_viewport_keeps_selection_visible_in_diff_pane() {
    let mut app = build_test_app();
    app.active_pane = ActivePane::Diff;
    app.diff_view = build_diff_view(120);

    let rendered_line_count = app.diff_view.display_line_count(DiffViewMode::Split, 160);
    app.selected_diff_line_index = rendered_line_count.saturating_sub(1);
    app.diff_scroll = 0;

    let viewport = app
        .prepare_diff_viewport(DiffViewMode::Split, 160, 12)
        .expect("viewport should be available");

    assert!(viewport.start <= viewport.selected_index);
    assert!(viewport.selected_index < viewport.end);
    assert!(app.diff_scroll > 0);
}

#[test]
fn prepare_diff_viewport_does_not_auto_scroll_from_sidebar_pane() {
    let mut app = build_test_app();
    app.active_pane = ActivePane::Sidebar;
    app.diff_view = build_diff_view(120);
    app.selected_diff_line_index = 40;
    app.diff_scroll = 0;

    let viewport = app
        .prepare_diff_viewport(DiffViewMode::Split, 160, 12)
        .expect("viewport should be available");

    assert_eq!(viewport.start, 0);
    assert_eq!(app.diff_scroll, 0);
}

#[test]
fn prepare_diff_viewport_counts_review_comment_rows() {
    let mut app = build_test_app();
    app.active_pane = ActivePane::Sidebar;
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/app.rs".to_string(),
        label: "app.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.diff_view = build_diff_view(1);
    app.review_report = Some(ReviewReport {
        summary: ReviewSummary {
            headline: "Needs review".to_string(),
            verdict: ReviewVerdict::HasConcerns,
            body: "One issue.".to_string(),
            risk_areas: Vec::new(),
        },
        findings: vec![ReviewFinding {
            path: "src/app.rs".to_string(),
            side: ReviewSide::New,
            line: Some(1),
            end_line: None,
            severity: ReviewSeverity::Medium,
            title: "Comment should affect scrolling".to_string(),
            body: "This is a long review comment that should add several visual rows to the diff viewport when the terminal is narrow.".to_string(),
            suggested_patch: None,
            state: ReviewFindingState::Open,
            fingerprint: String::new(),
        }],
    });

    let raw_line_count = app.diff_view.rendered_lines(DiffViewMode::Split, 48).len();
    let viewport = app
        .prepare_diff_viewport(DiffViewMode::Split, 48, 3)
        .expect("viewport should be available");

    assert!(viewport.rendered_line_count > raw_line_count);

    app.diff_scroll = u16::MAX;
    let viewport = app
        .prepare_diff_viewport(DiffViewMode::Split, 48, 3)
        .expect("viewport should be available");

    assert_eq!(
        app.diff_scroll as usize,
        viewport
            .rendered_line_count
            .saturating_sub(3)
            .min(u16::MAX as usize)
    );
}

#[test]
fn page_or_scroll_diff_moves_selection_when_diff_pane_is_active() {
    let mut app = build_test_app();
    app.active_pane = ActivePane::Diff;
    app.diff_view = build_diff_view(120);
    app.update_diff_viewport(DiffViewMode::Split, 160, 0, 12);

    let initial_selection = app
        .diff_view
        .first_selectable_index(DiffViewMode::Split, 160);
    app.selected_diff_line_index = initial_selection;

    app.page_or_scroll_diff(3);

    assert!(app.selected_diff_line_index > initial_selection);
}

#[test]
fn scroll_sidebar_saturates_at_zero() {
    let mut app = build_test_app();
    app.sidebar_scroll = 2;

    app.scroll_sidebar(-5);

    assert_eq!(app.sidebar_scroll, 0);
}

#[test]
fn build_diff_cache_key_includes_review_scope() {
    let file = FileEntry {
        status: "M ".to_string(),
        path: "src/app.rs".to_string(),
        label: "app.rs".to_string(),
        filetype: Some("rust"),
    };
    let commit_key = App::build_diff_cache_key(
        &ReviewMode::CommitCompare(CommitCompareSelection {
            base_ref: "base".to_string(),
            commit_hash: "commit".to_string(),
            short_hash: "abc123".to_string(),
            subject: "subject".to_string(),
        }),
        &file,
    );
    let branch_key = App::build_diff_cache_key(
        &ReviewMode::BranchCompare(BranchCompareSelection {
            source_ref: "feature".to_string(),
            destination_ref: "main".to_string(),
        }),
        &file,
    );

    assert_eq!(commit_key.review_scope, "commit:base:commit");
    assert_eq!(branch_key.review_scope, "branch:feature:main");
}
