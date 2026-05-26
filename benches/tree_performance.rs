use std::{collections::HashSet, fs, hint::black_box, path::PathBuf, sync::LazyLock};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use serde::Deserialize;
use vigil::{
    git::FileEntry,
    sidebar::{
        FileTreeOptions, FileTreeSearch, FileTreeSearchMode, FileTreeViewportMetrics,
        build_sidebar_items, build_sidebar_items_with_options, compute_window_range,
    },
};

#[derive(Debug)]
struct TreeFixture {
    pierre_entries: Vec<FileEntry>,
    linux_entries: Vec<FileEntry>,
}

#[derive(Deserialize)]
struct LinuxFixture {
    files: Vec<String>,
}

static TREE_FIXTURE: LazyLock<TreeFixture> = LazyLock::new(load_tree_fixture);

fn load_tree_fixture() -> TreeFixture {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let pierre_root = std::env::var("PIERRE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/scottfo".to_string());
            PathBuf::from(home).join("gitrepos/pierre")
        });
    let tree_data_root = pierre_root.join("packages/tree-test-data");

    let pierre_paths: Vec<String> = serde_json::from_str(
        &fs::read_to_string(tree_data_root.join("pierre-snapshot-files.json")).unwrap_or_else(
            |error| {
                panic!(
                    "failed to read Pierre tree fixture from {}: {error}",
                    repo_root.display()
                )
            },
        ),
    )
    .expect("Pierre snapshot tree fixture should parse");
    let linux_fixture: LinuxFixture =
        serde_json::from_str(&fs::read_to_string(tree_data_root.join("linux-files.json")).unwrap())
            .expect("Linux tree fixture should parse");

    TreeFixture {
        pierre_entries: file_entries(&unique_paths(pierre_paths)),
        linux_entries: file_entries(&unique_paths(linux_fixture.files)),
    }
}

fn unique_paths(mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths.dedup();
    paths
}

fn file_entries(paths: &[String]) -> Vec<FileEntry> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| FileEntry {
            status: match index % 7 {
                0 => "M ".to_string(),
                1 => "A ".to_string(),
                2 => "??".to_string(),
                _ => "  ".to_string(),
            },
            path: path.clone(),
            label: path.rsplit('/').next().unwrap_or(path.as_str()).to_string(),
            filetype: None,
        })
        .collect()
}

fn collapsed_roots(entries: &[FileEntry], count: usize) -> HashSet<String> {
    let mut roots = entries
        .iter()
        .filter_map(|entry| {
            entry
                .path
                .split_once('/')
                .map(|(root, _)| format!("{root}/"))
        })
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    roots.into_iter().take(count).collect()
}

fn bench_tree_pipeline(c: &mut Criterion) {
    let fixture = &*TREE_FIXTURE;
    let mut group = c.benchmark_group("tree_pipeline");
    group.sample_size(20);
    group.throughput(Throughput::Elements(fixture.linux_entries.len() as u64));

    group.bench_function("build_pierre_snapshot_open", |b| {
        b.iter(|| {
            let items = build_sidebar_items(black_box(&fixture.pierre_entries), &HashSet::new());
            black_box(items.len())
        });
    });

    group.bench_function("build_linux_open", |b| {
        b.iter(|| {
            let items = build_sidebar_items(black_box(&fixture.linux_entries), &HashSet::new());
            black_box(items.len())
        });
    });

    let collapsed = collapsed_roots(&fixture.linux_entries, 32);
    group.bench_function("build_linux_with_collapsed_roots", |b| {
        b.iter(|| {
            let items =
                build_sidebar_items(black_box(&fixture.linux_entries), black_box(&collapsed));
            black_box(items.len())
        });
    });

    let search_options = FileTreeOptions {
        search: Some(FileTreeSearch {
            query: "sched".to_string(),
            mode: FileTreeSearchMode::HideNonMatches,
        }),
        ..FileTreeOptions::default()
    };
    group.bench_function("build_linux_search_hide_non_matches", |b| {
        b.iter(|| {
            let items = build_sidebar_items_with_options(
                black_box(&fixture.linux_entries),
                &search_options,
            );
            black_box(items.len())
        });
    });

    let item_count = build_sidebar_items(&fixture.linux_entries, &HashSet::new()).len();
    group.bench_function("compute_window_range", |b| {
        b.iter(|| {
            compute_window_range(
                black_box(FileTreeViewportMetrics {
                    item_count,
                    item_height: 1,
                    scroll_top: 12_000,
                    viewport_height: 40,
                    overscan: 10,
                }),
                None,
            )
        });
    });

    group.finish();
}

criterion_group!(benches, bench_tree_pipeline);
criterion_main!(benches);
