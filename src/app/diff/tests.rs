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

fn build_large_review_snapshot(line_count: usize) -> git::ReviewDiffSnapshot {
    let mut diff = format!(
        "diff --git a/src/large.rs b/src/large.rs\n\
--- a/src/large.rs\n\
+++ b/src/large.rs\n\
@@ -0,0 +1,{line_count} @@\n"
    );
    for index in 0..line_count {
        diff.push_str(&format!(
            "+pub fn generated_line_{index}() -> usize {{ {index} }}\n"
        ));
    }

    git::ReviewDiffSnapshot::from_diff_text(&diff, Some("large-snapshot"))
        .expect("large snapshot fixture should parse")
}

fn build_review_text_index() -> git::ReviewDiffTextIndex {
    git::ReviewDiffTextIndex::from_diff_text_owned(
        concat!(
            "diff --git a/src/file-0.rs b/src/file-0.rs\n",
            "--- a/src/file-0.rs\n",
            "+++ b/src/file-0.rs\n",
            "@@ -1,1 +1,2 @@\n",
            " fn existing() {}\n",
            "+fn from_text_index() {}\n",
        )
        .to_string(),
    )
}

fn build_file_entries(count: usize) -> Vec<FileEntry> {
    (0..count)
        .map(|index| FileEntry {
            status: "M ".to_string(),
            path: format!("src/file-{index}.rs"),
            label: format!("file-{index}.rs"),
            filetype: Some("rust"),
        })
        .collect()
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
fn snapshot_prefetch_builds_plain_diff_without_syntax_highlight() {
    let key = build_cache_key(0);
    let event = build_snapshot_prefetch_event(
        12,
        key,
        build_file_entries(1).remove(0),
        Arc::new(build_review_snapshot()),
    )
    .expect("snapshot prefetch should build");

    assert_eq!(event.generation, 12);
    assert!(event.highlighted.is_none());
    assert!(!event.highlight_complete);
    assert!(event.plain.has_diff_rows());
}

#[tokio::test]
async fn diff_highlight_queues_full_file_job_while_sidebar_is_active() {
    let mut app = build_test_app();
    app.files = build_file_entries(1);
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    let key = app.diff_cache_key(&app.files[0]);
    app.pending_diff_cache_key = Some(key.clone());
    app.diff_view = build_diff_view(12);
    app.update_diff_viewport(DiffViewMode::Unified, 120, 0, 8);
    app.active_pane = ActivePane::Sidebar;
    app.diff_highlight_complete = false;
    app.highlight_registry = Some(
        git::HighlightRegistry::new_for_filetypes(["rust"])
            .expect("rust registry should initialize")
            .into(),
    );

    app.maybe_queue_diff_highlight();

    assert!(matches!(
        app.diff_highlight_job.as_ref(),
        Some(DiffHighlightJob {
            request_id,
            key: job_key,
            kind: DiffHighlightJobKind::Full,
        }) if *request_id == app.diff_request_id && *job_key == key
    ));
    app.cancel_inflight_diff_highlight();
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
    assert_eq!(
        app.diff_view
            .estimated_display_line_count(app.diff_view_mode, app.diff_line_wrap_mode),
        2
    );
}

#[test]
fn selected_large_diff_load_uses_review_snapshot_without_loading_task() {
    let mut app = build_test_app();
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/large.rs".to_string(),
        label: "large.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_snapshot = Some(Arc::new(
        build_large_review_snapshot(3_000).with_generation(app.diff_cache_generation),
    ));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_none());
    assert!(app.diff_view.note.is_none());
    assert!(app.diff_view.has_diff_rows());
}

#[test]
fn selected_diff_load_uses_review_text_index_without_task() {
    let mut app = build_test_app();
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_text_index = Some(Arc::new(build_review_text_index()));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_none());
    assert!(app.diff_view.note.is_none());
    assert!(app.diff_view.has_diff_rows());
}

#[tokio::test]
async fn selected_diff_load_uses_streamed_review_file_without_task() {
    let mut app = build_test_app();
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    let mut stream_index = git::ReviewDiffPartialTextIndex::default();
    stream_index.insert_file_diff(
        "src/file-0.rs".to_string(),
        concat!(
            "diff --git a/src/file-0.rs b/src/file-0.rs\n",
            "--- a/src/file-0.rs\n",
            "+++ b/src/file-0.rs\n",
            "@@ -1,1 +1,2 @@\n",
            " fn existing() {}\n",
            "+fn from_stream() {}\n",
        )
        .to_string(),
    );
    app.review_diff_stream_index = Some(stream_index);
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_none());
    assert!(app.diff_view.note.is_none());
    assert!(app.diff_view.has_diff_rows());
    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn working_tree_initial_file_loads_selected_preview_while_whole_diff_streams() {
    let mut app = build_test_app();
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_some());
    assert_eq!(app.diff_view.note.as_deref(), Some("Loading diff..."));

    app.cancel_inflight_diff_load();
    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn same_scope_navigation_waits_for_inflight_whole_diff_without_per_file_task() {
    let mut app = build_test_app();
    app.files = build_file_entries(2);
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.pending_diff_cache_key = Some(app.diff_cache_key(&app.files[0]));
    app.selected_file_index = 1;
    app.sync_sidebar_state();
    app.diff_view = build_diff_view(2);
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_none());
    assert!(!app.diff_view.has_diff_rows());
    assert_eq!(app.diff_view.note.as_deref(), Some(""));

    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn branch_compare_initial_file_loads_selected_preview_while_whole_diff_streams() {
    let mut app = build_test_app();
    app.review_mode = ReviewMode::BranchCompare(BranchCompareSelection {
        source_ref: "feature".to_string(),
        destination_ref: "main".to_string(),
    });
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_some());
    assert_eq!(app.diff_view.note.as_deref(), Some("Loading diff..."));

    app.cancel_inflight_diff_load();
    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn commit_compare_initial_file_loads_selected_preview_while_whole_diff_streams() {
    let mut app = build_test_app();
    app.review_mode = ReviewMode::CommitCompare(CommitCompareSelection {
        base_ref: "HEAD~1".to_string(),
        commit_hash: "HEAD".to_string(),
        short_hash: "abc123".to_string(),
        subject: "test commit".to_string(),
    });
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_some());
    assert_eq!(app.diff_view.note.as_deref(), Some("Loading diff..."));

    app.cancel_inflight_diff_load();
    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn branch_compare_same_scope_navigation_still_waits_for_whole_diff_stream() {
    let mut app = build_test_app();
    app.review_mode = ReviewMode::BranchCompare(BranchCompareSelection {
        source_ref: "feature".to_string(),
        destination_ref: "main".to_string(),
    });
    app.files = build_file_entries(2);
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.pending_diff_cache_key = Some(app.diff_cache_key(&app.files[0]));
    app.selected_file_index = 1;
    app.sync_sidebar_state();
    app.diff_view = DiffView::empty("");
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_none());
    assert_eq!(app.diff_view.note.as_deref(), Some(""));

    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn commit_refresh_keeps_selected_diff_visible_while_whole_diff_streams() {
    let mut app = build_test_app();
    app.review_mode = ReviewMode::CommitCompare(CommitCompareSelection {
        base_ref: "HEAD~1".to_string(),
        commit_hash: "HEAD".to_string(),
        short_hash: "abc123".to_string(),
        subject: "test commit".to_string(),
    });
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.pending_diff_cache_key = Some(app.diff_cache_key(&app.files[0]));
    app.diff_view = build_diff_view(2);
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_none());
    assert!(app.diff_view.has_diff_rows());
    assert!(app.diff_view.note.is_none());
    app.cancel_inflight_review_diff_snapshot();
}

#[test]
fn text_index_loaded_recovers_selected_diff_from_existing_plain_cache() {
    let mut app = build_test_app();
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    let cache_key = app.diff_cache_key(&app.files[0]);
    app.pending_diff_cache_key = Some(cache_key.clone());
    app.diff_view = DiffView::empty("");
    app.diff_view_cache
        .insert_plain(cache_key, build_diff_view(2));
    app.review_diff_text_index = Some(Arc::new(build_review_text_index()));

    assert!(app.load_selected_diff_from_review_text_index());
    assert!(app.diff_view.has_diff_rows());
    assert!(app.diff_view.note.is_none());
}

#[tokio::test]
async fn streamed_current_file_wakes_blank_diff_while_snapshot_is_inflight() {
    let mut app = build_test_app();
    app.files = vec![FileEntry {
        status: "M ".to_string(),
        path: "src/file-0.rs".to_string(),
        label: "file-0.rs".to_string(),
        filetype: Some("rust"),
    }];
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_snapshot_request_id = 9;
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.queue_selected_diff_load(true, true);

    assert!(!app.diff_view.has_diff_rows());

    let changed = app.handle_review_diff_file_streamed(
        9,
        app.diff_cache_generation,
        git::ReviewDiffStreamedFile {
            path: "src/file-0.rs".to_string(),
            diff: concat!(
                "diff --git a/src/file-0.rs b/src/file-0.rs\n",
                "--- a/src/file-0.rs\n",
                "+++ b/src/file-0.rs\n",
                "@@ -1,1 +1,2 @@\n",
                " fn existing() {}\n",
                "+fn from_stream() {}\n",
            )
            .to_string(),
        },
    );

    assert!(changed);
    assert!(app.diff_view.note.is_none());
    assert!(app.diff_view.has_diff_rows());
    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn streamed_visible_file_is_cached_without_per_file_load() {
    let mut app = build_test_app();
    app.files = build_file_entries(2);
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.sidebar_viewport_height = 20;
    app.review_diff_snapshot_request_id = 11;
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));
    let key = app.diff_cache_key(&app.files[1]);

    let changed = app.handle_review_diff_file_streamed(
        11,
        app.diff_cache_generation,
        git::ReviewDiffStreamedFile {
            path: "src/file-1.rs".to_string(),
            diff: concat!(
                "diff --git a/src/file-1.rs b/src/file-1.rs\n",
                "--- a/src/file-1.rs\n",
                "+++ b/src/file-1.rs\n",
                "@@ -1,1 +1,2 @@\n",
                " fn existing() {}\n",
                "+fn from_stream() {}\n",
            )
            .to_string(),
        },
    );

    assert!(!changed);
    assert!(app.diff_view_cache.has_plain(&key));
    assert!(app.diff_load_task.is_none());
    app.cancel_inflight_review_diff_snapshot();
}

#[test]
fn diff_stats_state_reports_review_snapshot_totals() {
    let mut app = build_test_app();
    app.review_diff_snapshot = Some(Arc::new(
        build_review_snapshot().with_generation(app.diff_cache_generation),
    ));

    let DiffStatsState::Ready(stats) = app.diff_stats_state() else {
        panic!("snapshot should produce ready diff stats");
    };

    assert_eq!(stats.file_count, 1);
    assert_eq!(stats.additions, 1);
    assert_eq!(stats.deletions, 0);
    assert_eq!(stats.lines, 2);
}

#[test]
fn diff_stats_state_reports_fast_stats_before_snapshot() {
    let mut app = build_test_app();
    app.review_diff_stats = Some(git::ReviewDiffStats {
        file_count: 3,
        additions: 21,
        deletions: 8,
        lines: 29,
        split_lines: 29,
    });

    let DiffStatsState::Ready(stats) = app.diff_stats_state() else {
        panic!("background stats should produce ready diff stats");
    };

    assert_eq!(stats.file_count, 3);
    assert_eq!(stats.additions, 21);
    assert_eq!(stats.deletions, 8);
    assert_eq!(stats.lines, 29);
}

#[tokio::test]
async fn selected_diff_load_keeps_current_view_while_uncached_file_loads() {
    let mut app = build_test_app();
    app.files = build_file_entries(2);
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.diff_view = build_diff_view(3);
    app.selected_file_index = 1;

    app.queue_selected_diff_load(true, true);

    assert!(app.diff_load_task.is_some());
    assert!(app.diff_view.note.is_none());
    assert!(app.diff_view.has_diff_rows());

    app.cancel_inflight_diff_load();
}

#[test]
fn directional_prefetch_prioritizes_movement_direction() {
    let mut app = build_test_app();
    app.files = build_file_entries(140);
    app.rebuild_sidebar_items();
    app.selected_file_index = 10;
    app.sync_sidebar_state();
    app.review_diff_snapshot = Some(Arc::new(git::ReviewDiffSnapshot::default()));
    app.diff_prefetch_direction = DiffPrefetchDirection::Forward;

    let visible_paths = app.visible_file_paths();
    let selected_visible_index = app
        .selected_visible_file_index()
        .expect("selected file should be visible");
    let prefetch_files = app.diff_prefetch_files(selected_visible_index, &visible_paths);

    assert_eq!(prefetch_files[0].1.path, "src/file-11.rs");
    assert_eq!(prefetch_files[1].1.path, "src/file-12.rs");
    assert!(
        prefetch_files
            .iter()
            .take(DIFF_DIRECTIONAL_PREFETCH_DISTANCE)
            .all(|(_, file)| file.path != "src/file-9.rs")
    );
    assert!(
        prefetch_files
            .iter()
            .any(|(_, file)| file.path == "src/file-9.rs")
    );
}

#[test]
fn visible_highlight_prefetch_prioritizes_cached_rows_by_distance() {
    let mut app = build_test_app();
    app.files = build_file_entries(8);
    app.rebuild_sidebar_items();
    app.selected_file_index = 3;
    app.sync_sidebar_state();
    app.sidebar_scroll = 0;
    app.sidebar_viewport_height = 10;
    app.diff_highlight_complete = true;
    app.highlight_registry = Some(
        git::HighlightRegistry::new_for_filetypes([])
            .expect("empty registry should initialize")
            .into(),
    );

    for index in [1, 2, 3, 4, 5] {
        app.diff_view_cache
            .insert_plain(app.diff_cache_key(&app.files[index]), build_diff_view(2));
    }

    let jobs = app.diff_visible_highlight_prefetch_files();
    let paths = jobs
        .iter()
        .map(|job| job.file.path.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![
            "src/file-2.rs",
            "src/file-4.rs",
            "src/file-1.rs",
            "src/file-5.rs"
        ]
    );
}

#[test]
fn visible_highlight_prefetch_waits_for_selected_highlight() {
    let mut app = build_test_app();
    app.files = build_file_entries(3);
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.sidebar_viewport_height = 10;
    app.diff_highlight_complete = false;
    app.highlight_registry = Some(
        git::HighlightRegistry::new_for_filetypes([])
            .expect("empty registry should initialize")
            .into(),
    );
    app.diff_view_cache
        .insert_plain(app.diff_cache_key(&app.files[1]), build_diff_view(2));

    assert!(app.diff_visible_highlight_prefetch_files().is_empty());

    app.diff_highlight_complete = true;

    assert_eq!(app.diff_visible_highlight_prefetch_files().len(), 1);
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
async fn diff_search_index_builds_from_review_text_index_before_snapshot() {
    let mut app = build_test_app();
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    assert!(!app.handle_review_diff_text_index_loaded(
        app.review_diff_snapshot_request_id,
        app.diff_cache_generation,
        Ok(Arc::new(build_review_text_index())),
    ));

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
    let index = result.expect("text index should produce a search index");
    assert_eq!(index.file_count(), 1);
    assert_eq!(index.line_count(), 2);
    assert!(
        app.review_diff_snapshot_task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    );

    app.cancel_inflight_review_diff_snapshot();
}

#[tokio::test]
async fn diff_search_modal_searches_streamed_partial_index_before_snapshot() {
    let mut app = build_test_app();
    app.files = build_file_entries(1);
    app.rebuild_sidebar_items();
    app.sync_sidebar_state();
    app.review_diff_snapshot_request_id = app.review_diff_snapshot_request_id.saturating_add(1);
    let request_id = app.review_diff_snapshot_request_id;
    app.review_diff_snapshot_task = Some(tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }));

    app.open_diff_search_modal();

    assert!(app.diff_search_modal_open);
    assert!(app.diff_search_loading);
    assert!(app.diff_search_load_task.is_none());

    let changed = app.handle_review_diff_file_streamed(
        request_id,
        app.diff_cache_generation,
        git::ReviewDiffStreamedFile {
            path: "src/file-0.rs".to_string(),
            diff: concat!(
                "diff --git a/src/file-0.rs b/src/file-0.rs\n",
                "--- a/src/file-0.rs\n",
                "+++ b/src/file-0.rs\n",
                "@@ -1,1 +1,1 @@\n",
                "-fn before() {}\n",
                "+fn streamed_partial_match() {}\n",
            )
            .to_string(),
        },
    );

    assert!(changed);
    assert!(app.diff_search_index.is_some());
    assert!(app.diff_search_is_indexing_partial());
    assert_eq!(
        app.diff_search_partial_loading_message(),
        "Searching indexed files, still indexing..."
    );

    for ch in "streamed".chars() {
        assert!(
            app.handle_diff_search_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(ch),
                crossterm::event::KeyModifiers::NONE,
            ))
            .await
            .expect("diff search key should be handled")
        );
    }

    let query_request_id = app.diff_search_query_request_id;
    let results = loop {
        let event = app
            .events
            .next()
            .await
            .expect("search results event should be emitted");
        let Event::DiffSearchResultsLoaded { request_id, result } = event else {
            continue;
        };
        if request_id == query_request_id {
            break result.expect("partial search should succeed");
        }
    };
    assert_eq!(results.items.len(), 1);
    assert_eq!(results.items[0].file_path, "src/file-0.rs");
    assert!(results.items[0].line.contains("streamed_partial_match"));

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

    let rendered_line_count =
        app.diff_view
            .display_line_count(DiffViewMode::Split, 160, app.diff_line_wrap_mode);
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

    let raw_line_count = app
        .diff_view
        .rendered_lines(DiffViewMode::Split, 48, app.diff_line_wrap_mode)
        .len();
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

    let initial_selection =
        app.diff_view
            .first_selectable_index(DiffViewMode::Split, 160, app.diff_line_wrap_mode);
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
