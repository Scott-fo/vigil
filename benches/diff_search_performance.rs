use std::{hint::black_box, sync::LazyLock};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use vigil::git::{DiffSearchIndex, DiffSearchMatcher, DiffSearchMode, DiffSearchOptions};

const FILE_COUNT: usize = 1_000;
const LINES_PER_FILE: usize = 1_000;
const MILLION_LINE_COUNT: usize = FILE_COUNT * LINES_PER_FILE;
const SEARCH_LIMIT: usize = 50;

static MILLION_LINE_DIFF: LazyLock<String> = LazyLock::new(build_million_line_diff);
static MILLION_LINE_INDEX: LazyLock<DiffSearchIndex> = LazyLock::new(|| {
    DiffSearchIndex::from_diff_text(&build_million_line_diff())
        .expect("million-line diff fixture should parse")
});

fn build_million_line_diff() -> String {
    let mut diff = String::with_capacity(MILLION_LINE_COUNT * 96);

    for file_index in 0..FILE_COUNT {
        let path = format!("src/generated/module_{file_index:04}.rs");
        diff.push_str(&format!(
            "diff --git a/{path} b/{path}\n\
index 0000000..1111111 100644\n\
--- /dev/null\n\
+++ b/{path}\n\
@@ -0,0 +1,{LINES_PER_FILE} @@\n"
        ));

        for line_index in 0..LINES_PER_FILE {
            let global_index = file_index * LINES_PER_FILE + line_index;
            if global_index % 997 == 0 {
                diff.push_str(&format!(
                    "+pub fn target_search_needle_{global_index}() -> usize {{ {global_index} }}\n"
                ));
            } else if line_index % 7 == 0 {
                diff.push_str(&format!(
                    "+let render_status_line_{global_index} = format!(\"module={file_index} line={line_index} state={{}}\", state);\n"
                ));
            } else {
                diff.push_str(&format!(
                    "+let generated_value_{global_index} = module_state.apply_delta({line_index});\n"
                ));
            }
        }
    }

    diff
}

fn bench_diff_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_search_million_lines");
    group.sample_size(10);
    group.throughput(Throughput::Elements(MILLION_LINE_COUNT as u64));

    group.bench_function("build_index_1m_added_lines", |b| {
        b.iter(|| {
            let index = DiffSearchIndex::from_diff_text(black_box(&MILLION_LINE_DIFF))
                .expect("million-line diff fixture should parse");
            black_box(index.line_count());
        });
    });

    let index = &*MILLION_LINE_INDEX;
    assert_eq!(index.file_count(), FILE_COUNT);
    assert_eq!(index.line_count(), MILLION_LINE_COUNT);

    let options = DiffSearchOptions {
        limit: SEARCH_LIMIT,
        include_context: true,
        ..DiffSearchOptions::default()
    };

    group.bench_function("search_top_50_common_query", |b| {
        let mut matcher = DiffSearchMatcher::default();
        b.iter(|| {
            let results = index.search(black_box("render status line"), options, &mut matcher);
            black_box(results.items.len());
        });
    });

    group.bench_function("search_top_50_sparse_query", |b| {
        let mut matcher = DiffSearchMatcher::default();
        b.iter(|| {
            let results = index.search(black_box("target needle 997000"), options, &mut matcher);
            black_box(results.items.len());
        });
    });

    let fuzzy_options = DiffSearchOptions {
        mode: DiffSearchMode::Fuzzy,
        ..options
    };

    group.bench_function("search_top_50_fuzzy_query", |b| {
        let mut matcher = DiffSearchMatcher::default();
        b.iter(|| {
            let results =
                index.search(black_box("render status line"), fuzzy_options, &mut matcher);
            black_box(results.items.len());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_diff_search);
criterion_main!(benches);
