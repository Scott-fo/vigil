use std::{hint::black_box, sync::LazyLock};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use vigil::{
    app::DiffViewMode,
    git::{
        FileDiffMetadata, build_diff_view_from_diff_text, build_diff_view_from_file_metadata,
        parse_patch_files,
    },
};

const FILE_COUNT: usize = 300;
const HUNKS_PER_FILE: usize = 4;
const SECTIONS_PER_HUNK: usize = 10;
const CONTEXT_LINES_PER_SECTION: usize = 3;
const REMOVED_LINES_PER_SECTION: usize = 2;
const ADDED_LINES_PER_SECTION: usize = 3;
const GAP_SIZE: usize = 24;
const DISPLAY_WIDTH: usize = 120;

static MULTI_FILE_DIFF: LazyLock<MultiFileDiffFixture> = LazyLock::new(build_multi_file_diff);
static PARSED_FILES: LazyLock<Vec<FileDiffMetadata>> = LazyLock::new(|| {
    parse_patch_files(&MULTI_FILE_DIFF.patch, Some("multi-diff"), true)
        .expect("multi-file patch fixture should parse")
        .into_iter()
        .flat_map(|patch| patch.files)
        .collect()
});

struct MultiFileDiffFixture {
    patch: String,
    file_patches: Vec<String>,
    line_count: usize,
}

fn build_multi_file_diff() -> MultiFileDiffFixture {
    let mut patch = String::with_capacity(FILE_COUNT * HUNKS_PER_FILE * SECTIONS_PER_HUNK * 512);
    let mut file_patches = Vec::with_capacity(FILE_COUNT);
    let mut line_count = 0;

    for file_index in 0..FILE_COUNT {
        let path = format!("src/review/module_{file_index:04}.rs");
        let mut file_patch = format!(
            "diff --git a/{path} b/{path}\n\
index 0000000..1111111 100644\n\
--- a/{path}\n\
+++ b/{path}\n"
        );
        let mut old_start = 1usize;
        let mut new_start = 1usize;

        for hunk_index in 0..HUNKS_PER_FILE {
            let mut hunk_lines = Vec::new();
            let mut old_count = 0usize;
            let mut new_count = 0usize;

            for section_index in 0..SECTIONS_PER_HUNK {
                let global_index = file_index * HUNKS_PER_FILE * SECTIONS_PER_HUNK
                    + hunk_index * SECTIONS_PER_HUNK
                    + section_index;

                for context_index in 0..CONTEXT_LINES_PER_SECTION {
                    hunk_lines.push(format!(
                        " pub fn stable_context_{global_index}_{context_index}() -> usize {{ {global_index} + {context_index} }}"
                    ));
                    old_count += 1;
                    new_count += 1;
                    line_count += 1;
                }

                for removed_index in 0..REMOVED_LINES_PER_SECTION {
                    hunk_lines.push(format!(
                        "-let legacy_value_{global_index}_{removed_index} = legacy_state.compute({removed_index});"
                    ));
                    old_count += 1;
                    line_count += 1;
                }

                for added_index in 0..ADDED_LINES_PER_SECTION {
                    hunk_lines.push(format!(
                        "+let reviewed_value_{global_index}_{added_index} = review_state.compute_with_cache({added_index});"
                    ));
                    new_count += 1;
                    line_count += 1;
                }
            }

            file_patch.push_str(&format!(
                "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
            ));
            file_patch.push_str(&hunk_lines.join("\n"));
            file_patch.push('\n');
            old_start += old_count + GAP_SIZE;
            new_start += new_count + GAP_SIZE;
        }

        patch.push_str(&file_patch);
        file_patches.push(file_patch);
    }

    MultiFileDiffFixture {
        patch,
        file_patches,
        line_count,
    }
}

fn rendered_line_count(mut views: impl Iterator<Item = vigil::git::DiffView>) -> usize {
    let mut total = 0usize;
    for mut view in views.by_ref() {
        total += view.display_line_count(DiffViewMode::Unified, DISPLAY_WIDTH);
    }
    total
}

fn bench_multi_diff_loading(c: &mut Criterion) {
    let fixture = &*MULTI_FILE_DIFF;
    assert_eq!(fixture.file_patches.len(), FILE_COUNT);
    assert_eq!(PARSED_FILES.len(), FILE_COUNT);

    let mut group = c.benchmark_group("multi_diff_loading");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(fixture.patch.len() as u64));

    group.bench_function("parse_patch_files_multi_file", |b| {
        b.iter(|| {
            let parsed = parse_patch_files(black_box(&fixture.patch), Some("multi-diff"), true)
                .expect("multi-file patch fixture should parse");
            black_box(parsed.len());
        });
    });

    group.bench_function("build_views_from_parsed_metadata", |b| {
        b.iter(|| {
            let line_count = rendered_line_count(
                PARSED_FILES
                    .iter()
                    .map(|file| build_diff_view_from_file_metadata(black_box(file))),
            );
            black_box(line_count);
        });
    });

    group.bench_function("build_views_from_file_diff_text", |b| {
        b.iter(|| {
            let line_count = rendered_line_count(
                fixture
                    .file_patches
                    .iter()
                    .map(|patch| build_diff_view_from_diff_text(black_box(patch), Some("rust"))),
            );
            black_box(line_count);
        });
    });

    group.bench_function("parse_once_then_build_views", |b| {
        b.iter(|| {
            let parsed = parse_patch_files(black_box(&fixture.patch), Some("multi-diff"), true)
                .expect("multi-file patch fixture should parse");
            let line_count = rendered_line_count(
                parsed
                    .iter()
                    .flat_map(|patch| patch.files.iter())
                    .map(|file| build_diff_view_from_file_metadata(black_box(file))),
            );
            black_box(line_count);
        });
    });

    group.bench_function("build_one_view_from_whole_patch_text", |b| {
        b.iter(|| {
            let mut view = build_diff_view_from_diff_text(black_box(&fixture.patch), Some("rust"));
            black_box(view.display_line_count(DiffViewMode::Unified, DISPLAY_WIDTH));
        });
    });

    group.finish();

    black_box(fixture.line_count);
}

criterion_group!(benches, bench_multi_diff_loading);
criterion_main!(benches);
