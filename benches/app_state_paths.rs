use std::{fs, hint::black_box, path::PathBuf, process::Command, sync::LazyLock};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use vigil::{
    app::{ActivePane, App, BranchCompareField, DiffViewMode},
    git::{CommitSearchEntry, build_diff_view_from_diff_text},
};

const SPLIT_RENDER_WIDTH: usize = 160;
const VIEWPORT_HEIGHT: usize = 40;
const FILETYPE: Option<&'static str> = Some("rust");

struct AppStateFixture {
    repo_root: PathBuf,
    diff_text: String,
    commit_search_entries: Vec<CommitSearchEntry>,
    branch_refs: Vec<String>,
}

static APP_STATE_FIXTURE: LazyLock<AppStateFixture> = LazyLock::new(build_fixture);

fn build_fixture() -> AppStateFixture {
    let repo_root = std::env::temp_dir().join(format!("vigil-app-bench-{}", std::process::id()));
    fs::create_dir_all(&repo_root).expect("bench repo dir should exist");
    let status = Command::new("git")
        .arg("init")
        .arg("-q")
        .arg(&repo_root)
        .status()
        .expect("git init should run");
    assert!(status.success(), "git init should succeed");

    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/test_fixture.rs.txt");
    let content = fs::read_to_string(&source_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()))
        .replace("\r\n", "\n");

    let line_count = content.lines().count();
    let mut diff_text = format!(
        "diff --git a/src/app.rs.copy.rs b/src/app.rs.copy.rs\n\
new file mode 100644\n\
index 0000000..1111111\n\
--- /dev/null\n\
+++ b/src/app.rs.copy.rs\n\
@@ -0,0 +1,{} @@\n",
        line_count
    );
    for line in content.lines() {
        diff_text.push('+');
        diff_text.push_str(line);
        diff_text.push('\n');
    }

    let commit_search_entries = (0usize..512)
        .map(|index| CommitSearchEntry {
            hash: format!("{index:040x}"),
            short_hash: format!("{:07x}", index),
            parent_hashes: vec![format!("{:040x}", index.saturating_sub(1))],
            author: "Bench Author".to_string(),
            date: "2026-03-24".to_string(),
            subject: if index % 16 == 0 {
                format!("refactor diff viewport path {index}")
            } else if index % 7 == 0 {
                format!("optimize branch compare filtering {index}")
            } else {
                format!("update app flow {index}")
            },
        })
        .collect();

    let branch_refs = (0usize..512)
        .map(|index| {
            if index == 0 {
                "main".to_string()
            } else if index == 1 {
                "master".to_string()
            } else if index % 9 == 0 {
                format!("release/{index}")
            } else {
                format!("feature/refactor-{index}")
            }
        })
        .collect();

    AppStateFixture {
        repo_root,
        diff_text,
        commit_search_entries,
        branch_refs,
    }
}

fn build_benchmark_app(fixture: &AppStateFixture) -> App {
    let mut app = App::new_for_benchmarks(fixture.repo_root.clone());
    app.diff_view = build_diff_view_from_diff_text(&fixture.diff_text, FILETYPE);
    app.commit_search_entries = fixture.commit_search_entries.clone();
    app.branch_compare_available_refs = fixture.branch_refs.clone();
    app
}

fn bench_app_state_paths(c: &mut Criterion) {
    let fixture = &*APP_STATE_FIXTURE;
    let mut viewport_app = build_benchmark_app(fixture);
    let mut commit_search_app = build_benchmark_app(fixture);
    let mut branch_compare_app = build_benchmark_app(fixture);
    let mut theme_filter_app = build_benchmark_app(fixture);
    let rendered_line_count = viewport_app
        .diff_view
        .display_line_count(DiffViewMode::Split);
    let mut diff_cursor = 0usize;
    let mut sidebar_cursor = rendered_line_count / 3;

    commit_search_app.commit_search_query = "refactor viewport".to_string();
    branch_compare_app.branch_compare_active_field = BranchCompareField::Destination;
    branch_compare_app.branch_compare_destination_query = "release".to_string();
    theme_filter_app.theme_modal_query = "cat".to_string();

    let mut group = c.benchmark_group("app_state_paths");
    group.sample_size(20);
    group.throughput(Throughput::Elements(rendered_line_count as u64));

    group.bench_function("prepare_diff_viewport_diff_pane", |b| {
        b.iter(|| {
            diff_cursor = (diff_cursor + 17) % rendered_line_count.max(1);
            viewport_app.active_pane = ActivePane::Diff;
            viewport_app.diff_scroll = (diff_cursor / 3).min(u16::MAX as usize) as u16;
            viewport_app.selected_diff_line_index = black_box(diff_cursor);
            let viewport = viewport_app.prepare_diff_viewport(
                DiffViewMode::Split,
                black_box(SPLIT_RENDER_WIDTH),
                black_box(VIEWPORT_HEIGHT),
            );
            black_box(viewport);
        });
    });

    group.bench_function("prepare_diff_viewport_sidebar_pane", |b| {
        b.iter(|| {
            sidebar_cursor = (sidebar_cursor + 23) % rendered_line_count.max(1);
            viewport_app.active_pane = ActivePane::Sidebar;
            viewport_app.diff_scroll = (sidebar_cursor / 4).min(u16::MAX as usize) as u16;
            viewport_app.selected_diff_line_index = black_box(sidebar_cursor);
            let viewport = viewport_app.prepare_diff_viewport(
                DiffViewMode::Split,
                black_box(SPLIT_RENDER_WIDTH),
                black_box(VIEWPORT_HEIGHT),
            );
            black_box(viewport);
        });
    });

    group.bench_function("filtered_commit_search_indices", |b| {
        b.iter(|| {
            let indices = commit_search_app.filtered_commit_search_indices();
            black_box(indices);
        });
    });

    group.bench_function("filtered_branch_compare_refs", |b| {
        b.iter(|| {
            let refs = branch_compare_app.filtered_branch_compare_refs();
            black_box(refs);
        });
    });

    group.bench_function("filtered_theme_names", |b| {
        b.iter(|| {
            let names = theme_filter_app.filtered_theme_names();
            black_box(names);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_app_state_paths);
criterion_main!(benches);
