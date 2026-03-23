use std::{fs, hint::black_box, path::PathBuf, sync::LazyLock};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use vigil::{
    app::DiffViewMode,
    git::{
        HighlightRegistry, build_diff_view_from_diff_text,
        build_diff_view_from_diff_text_with_context, clear_exact_highlight_cache,
    },
};

const FILETYPE: Option<&'static str> = Some("rust");
const SPLIT_RENDER_WIDTH: usize = 160;
const VIEWPORT_HEIGHT: usize = 40;

struct GitRsNewFileFixture {
    diff: String,
    new_file_lines: Vec<String>,
}

static GIT_RS_NEW_FILE_FIXTURE: LazyLock<GitRsNewFileFixture> = LazyLock::new(build_fixture);

fn build_fixture() -> GitRsNewFileFixture {
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/git.rs");
    let content = fs::read_to_string(&source_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()))
        .replace("\r\n", "\n");

    let mut new_file_lines = content
        .split('\n')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if content.ends_with('\n') {
        let _ = new_file_lines.pop();
    }

    let mut diff = format!(
        "diff --git a/src/git.rs.copy.rs b/src/git.rs.copy.rs\n\
new file mode 100644\n\
index 0000000..1111111\n\
--- /dev/null\n\
+++ b/src/git.rs.copy.rs\n\
@@ -0,0 +1,{} @@\n",
        new_file_lines.len()
    );
    for line in &new_file_lines {
        diff.push('+');
        diff.push_str(line);
        diff.push('\n');
    }

    GitRsNewFileFixture {
        diff,
        new_file_lines,
    }
}

fn bench_git_rs_new_file_pipeline(c: &mut Criterion) {
    let fixture = &*GIT_RS_NEW_FILE_FIXTURE;
    let diff = &fixture.diff;
    let registry = HighlightRegistry::new().expect("highlight registry should initialize");
    let plain_view = build_diff_view_from_diff_text(diff, FILETYPE);
    let exact_context_view = build_diff_view_from_diff_text_with_context(
        diff,
        FILETYPE,
        None,
        Some(fixture.new_file_lines.clone()),
    );
    let display_line_count = plain_view.clone().display_line_count(DiffViewMode::Split);
    let scrolled_viewport_start = display_line_count / 2;
    let scrolled_viewport_end = (scrolled_viewport_start + VIEWPORT_HEIGHT).min(display_line_count);

    let mut group = c.benchmark_group("git_rs_new_file_pipeline");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(diff.len() as u64));

    group.bench_function("build_plain_view", |b| {
        b.iter(|| {
            let view = build_diff_view_from_diff_text(black_box(diff), FILETYPE);
            black_box(view);
        });
    });

    group.bench_function("highlight_visible_split_view", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |mut view| {
                view.apply_syntax_highlighting_for_display_range(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    0,
                    VIEWPORT_HEIGHT,
                    FILETYPE,
                    &registry,
                );
                black_box(view.display_line_count(DiffViewMode::Split));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("highlight_visible_split_view_scrolled", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |mut view| {
                view.apply_syntax_highlighting_for_display_range(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    scrolled_viewport_start,
                    scrolled_viewport_end,
                    FILETYPE,
                    &registry,
                );
                black_box(view.display_line_count(DiffViewMode::Split));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("highlight_exact_full_file_warm", |b| {
        b.iter_batched(
            || exact_context_view.clone(),
            |mut view| {
                view.apply_exact_syntax_highlighting(FILETYPE, &registry);
                black_box(view.display_line_count(DiffViewMode::Split));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("highlight_exact_full_file_cold", |b| {
        b.iter_batched(
            || {
                clear_exact_highlight_cache();
                exact_context_view.clone()
            },
            |mut view| {
                view.apply_exact_syntax_highlighting(FILETYPE, &registry);
                black_box(view.display_line_count(DiffViewMode::Split));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("initial_viewport_pipeline_split", |b| {
        b.iter(|| {
            let mut view = build_diff_view_from_diff_text(black_box(diff), FILETYPE);
            view.apply_syntax_highlighting_for_display_range(
                DiffViewMode::Split,
                SPLIT_RENDER_WIDTH,
                0,
                VIEWPORT_HEIGHT,
                FILETYPE,
                &registry,
            );
            let lines = view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH);
            black_box(lines.len());
        });
    });

    group.bench_function("exact_full_pipeline_split_warm", |b| {
        b.iter(|| {
            let mut view = build_diff_view_from_diff_text_with_context(
                black_box(diff),
                FILETYPE,
                None,
                Some(fixture.new_file_lines.clone()),
            );
            view.apply_exact_syntax_highlighting(FILETYPE, &registry);
            let lines = view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH);
            black_box(lines.len());
        });
    });

    group.bench_function("exact_full_pipeline_split_cold", |b| {
        b.iter(|| {
            clear_exact_highlight_cache();
            let mut view = build_diff_view_from_diff_text_with_context(
                black_box(diff),
                FILETYPE,
                None,
                Some(fixture.new_file_lines.clone()),
            );
            view.apply_exact_syntax_highlighting(FILETYPE, &registry);
            let lines = view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH);
            black_box(lines.len());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_git_rs_new_file_pipeline);
criterion_main!(benches);
