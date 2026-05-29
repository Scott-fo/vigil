use std::path::PathBuf;

use crate::event::DiffPrefetchedEvent;

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
