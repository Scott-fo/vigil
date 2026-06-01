use std::{hint::black_box, sync::LazyLock, time::Duration};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use vigil::{
    app::{DiffLineWrapMode, DiffViewMode},
    git::{DiffView, FileEntry, HighlightRegistry, ReviewDiffSnapshot},
};

const HUGE_REVIEW_FILE_COUNT: usize = 22_000;
const HUGE_REVIEW_SECTIONS_PER_FILE: usize = 10;
const HUGE_REVIEW_DIFF_LINES_PER_SECTION: usize = 5;
const SCROLL_SAMPLE_FILES: usize = 512;
const HIGHLIGHT_SAMPLE_FILES: usize = 128;
const LARGE_FILE_SECTIONS: usize = 5_000;
const SPLIT_RENDER_WIDTH: usize = 160;
const VIEWPORT_HEIGHT: usize = 40;
const FILETYPE: Option<&'static str> = Some("rust");

static HUGE_REVIEW: LazyLock<HugeReviewFixture> = LazyLock::new(build_huge_review_fixture);
static HUGE_REVIEW_SNAPSHOT: LazyLock<ReviewDiffSnapshot> = LazyLock::new(|| {
    ReviewDiffSnapshot::from_diff_text(&HUGE_REVIEW.patch, Some("huge-review"))
        .expect("huge review fixture should parse")
});
static LARGE_FILE: LazyLock<LargeFileFixture> = LazyLock::new(build_large_file_fixture);
static LARGE_FILE_SNAPSHOT: LazyLock<ReviewDiffSnapshot> = LazyLock::new(|| {
    ReviewDiffSnapshot::from_diff_text(&LARGE_FILE.patch, Some("large-file"))
        .expect("large file fixture should parse")
});
static HIGHLIGHT_REGISTRY: LazyLock<HighlightRegistry> = LazyLock::new(|| {
    HighlightRegistry::new_for_filetypes(["rust"]).expect("rust registry should initialize")
});

struct HugeReviewFixture {
    patch: String,
    files: Vec<FileEntry>,
    diff_line_count: usize,
}

struct LargeFileFixture {
    patch: String,
    file: FileEntry,
}

fn build_huge_review_fixture() -> HugeReviewFixture {
    let diff_lines =
        HUGE_REVIEW_FILE_COUNT * HUGE_REVIEW_SECTIONS_PER_FILE * HUGE_REVIEW_DIFF_LINES_PER_SECTION;
    let mut patch = String::with_capacity(diff_lines * 80);
    let mut files = Vec::with_capacity(HUGE_REVIEW_FILE_COUNT);

    for file_index in 0..HUGE_REVIEW_FILE_COUNT {
        let path = format!("src/review/module_{file_index:05}.rs");
        push_file_header(&mut patch, &path);

        let old_count = HUGE_REVIEW_SECTIONS_PER_FILE * 3;
        let new_count = HUGE_REVIEW_SECTIONS_PER_FILE * 4;
        patch.push_str(&format!("@@ -1,{old_count} +1,{new_count} @@\n"));

        for section_index in 0..HUGE_REVIEW_SECTIONS_PER_FILE {
            patch.push_str(&format!(
                " const stable_before_{file_index}_{section_index}: usize = {section_index};\n"
            ));
            patch.push_str(&format!(
                "-let legacy_value_{file_index}_{section_index} = old_state.compute({section_index});\n"
            ));
            patch.push_str(&format!(
                "+let reviewed_value_{file_index}_{section_index} = new_state.compute({section_index});\n"
            ));
            patch.push_str(&format!(
                "+let cached_value_{file_index}_{section_index} = reviewed_value_{file_index}_{section_index};\n"
            ));
            patch.push_str(&format!(
                " const stable_after_{file_index}_{section_index}: usize = {section_index} + 1;\n"
            ));
        }

        files.push(FileEntry {
            status: "M ".to_string(),
            path: path.clone(),
            label: format!("module_{file_index:05}.rs"),
            filetype: FILETYPE,
        });
    }

    HugeReviewFixture {
        patch,
        files,
        diff_line_count: diff_lines,
    }
}

fn build_large_file_fixture() -> LargeFileFixture {
    let path = "src/review/very_large_module.rs";
    let mut patch = String::with_capacity(LARGE_FILE_SECTIONS * 120);
    push_file_header(&mut patch, path);

    let old_count = LARGE_FILE_SECTIONS * 3;
    let new_count = LARGE_FILE_SECTIONS * 4;
    patch.push_str(&format!("@@ -1,{old_count} +1,{new_count} @@\n"));

    for section_index in 0..LARGE_FILE_SECTIONS {
        patch.push_str(&format!(
            " pub fn stable_context_{section_index}(input: usize) -> usize {{ input + {section_index} }}\n"
        ));
        patch.push_str(&format!(
            "-let legacy_dashboard_value_{section_index} = legacy_state.compute(input, {section_index});\n"
        ));
        patch.push_str(&format!(
            "+let reviewed_dashboard_value_{section_index} = review_state.compute_with_cache(input, {section_index});\n"
        ));
        patch.push_str(&format!(
            "+let final_dashboard_value_{section_index} = reviewed_dashboard_value_{section_index}.saturating_add(1);\n"
        ));
        patch.push_str(&format!(
            " pub fn stable_tail_{section_index}(input: usize) -> usize {{ input.saturating_sub({section_index}) }}\n"
        ));
    }

    LargeFileFixture {
        patch,
        file: FileEntry {
            status: "M ".to_string(),
            path: path.to_string(),
            label: "very_large_module.rs".to_string(),
            filetype: FILETYPE,
        },
    }
}

fn push_file_header(patch: &mut String, path: &str) {
    patch.push_str(&format!(
        "diff --git a/{path} b/{path}\n\
index 0000000..1111111 100644\n\
--- a/{path}\n\
+++ b/{path}\n"
    ));
}

fn split_viewport_line_count(view: &DiffView, line_wrap: DiffLineWrapMode) -> usize {
    view.rendered_lines_window(
        DiffViewMode::Split,
        SPLIT_RENDER_WIDTH,
        line_wrap,
        0,
        VIEWPORT_HEIGHT,
    )
    .len()
}

fn visible_highlight_line_count(mut view: DiffView, line_wrap: DiffLineWrapMode) -> usize {
    view.apply_syntax_highlighting_for_display_range(
        DiffViewMode::Split,
        SPLIT_RENDER_WIDTH,
        line_wrap,
        0,
        VIEWPORT_HEIGHT,
        FILETYPE,
        &HIGHLIGHT_REGISTRY,
    );
    split_viewport_line_count(&view, line_wrap)
}

fn bench_huge_review_scrolling(c: &mut Criterion) {
    let fixture = &*HUGE_REVIEW;
    assert_eq!(fixture.files.len(), HUGE_REVIEW_FILE_COUNT);
    assert!(fixture.diff_line_count >= 1_000_000);

    let snapshot = &*HUGE_REVIEW_SNAPSHOT;
    assert_eq!(snapshot.file_count(), HUGE_REVIEW_FILE_COUNT);

    let selected_file = &fixture.files[HUGE_REVIEW_FILE_COUNT / 2];
    let scroll_files = &fixture.files[0..SCROLL_SAMPLE_FILES];
    let highlight_files = &fixture.files[0..HIGHLIGHT_SAMPLE_FILES];

    let mut group = c.benchmark_group("diff_scrolling_huge_review");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));

    group.throughput(Throughput::Bytes(fixture.patch.len() as u64));
    group.bench_function("build_snapshot_22k_files_1m_lines", |b| {
        b.iter(|| {
            let snapshot =
                ReviewDiffSnapshot::from_diff_text(black_box(&fixture.patch), Some("bench"))
                    .expect("huge review fixture should parse");
            black_box((snapshot.file_count(), snapshot.line_count()));
        });
    });

    group.throughput(Throughput::Elements(1));
    group.bench_function("snapshot_plain_view_selected_file", |b| {
        b.iter(|| {
            let view = snapshot
                .build_diff_view(black_box(selected_file))
                .expect("selected file should exist in snapshot");
            black_box(view);
        });
    });

    group.throughput(Throughput::Elements(SCROLL_SAMPLE_FILES as u64));
    group.bench_function("snapshot_plain_views_512_files", |b| {
        b.iter(|| {
            let mut rows = 0usize;
            for file in scroll_files {
                let mut view = snapshot
                    .build_diff_view(black_box(file))
                    .expect("scroll file should exist in snapshot");
                rows = rows.saturating_add(view.display_line_count(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    DiffLineWrapMode::NoWrap,
                ));
            }
            black_box(rows);
        });
    });

    group.bench_function("snapshot_first_viewport_512_files_nowrap", |b| {
        b.iter(|| {
            let mut rows = 0usize;
            for file in scroll_files {
                let view = snapshot
                    .build_diff_view(black_box(file))
                    .expect("scroll file should exist in snapshot");
                rows =
                    rows.saturating_add(split_viewport_line_count(&view, DiffLineWrapMode::NoWrap));
            }
            black_box(rows);
        });
    });

    group.bench_function("snapshot_first_viewport_512_files_wrap", |b| {
        b.iter(|| {
            let mut rows = 0usize;
            for file in scroll_files {
                let view = snapshot
                    .build_diff_view(black_box(file))
                    .expect("scroll file should exist in snapshot");
                rows =
                    rows.saturating_add(split_viewport_line_count(&view, DiffLineWrapMode::Wrap));
            }
            black_box(rows);
        });
    });

    group.throughput(Throughput::Elements(HIGHLIGHT_SAMPLE_FILES as u64));
    group.bench_function("visible_highlight_128_files_nowrap", |b| {
        b.iter_batched(
            || {
                highlight_files
                    .iter()
                    .map(|file| {
                        snapshot
                            .build_diff_view(file)
                            .expect("highlight file should exist in snapshot")
                    })
                    .collect::<Vec<_>>()
            },
            |views| {
                let rows = views
                    .into_iter()
                    .map(|view| visible_highlight_line_count(view, DiffLineWrapMode::NoWrap))
                    .sum::<usize>();
                black_box(rows);
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

fn bench_large_file(c: &mut Criterion) {
    let fixture = &*LARGE_FILE;
    let snapshot = &*LARGE_FILE_SNAPSHOT;
    assert_eq!(snapshot.file_count(), 1);

    let mut group = c.benchmark_group("diff_scrolling_large_file");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));
    group.throughput(Throughput::Bytes(fixture.patch.len() as u64));

    group.bench_function("snapshot_plain_view_large_file", |b| {
        b.iter(|| {
            let view = snapshot
                .build_diff_view(black_box(&fixture.file))
                .expect("large file should exist in snapshot");
            black_box(view);
        });
    });

    group.bench_function("snapshot_first_viewport_large_file_nowrap", |b| {
        b.iter_batched(
            || {
                snapshot
                    .build_diff_view(&fixture.file)
                    .expect("large file should exist in snapshot")
            },
            |view| {
                black_box(split_viewport_line_count(&view, DiffLineWrapMode::NoWrap));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("visible_highlight_large_file_nowrap", |b| {
        b.iter_batched(
            || {
                snapshot
                    .build_diff_view(&fixture.file)
                    .expect("large file should exist in snapshot")
            },
            |view| {
                black_box(visible_highlight_line_count(view, DiffLineWrapMode::NoWrap));
            },
            BatchSize::LargeInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_huge_review_scrolling, bench_large_file);
criterion_main!(benches);
