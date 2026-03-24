use std::{
    fs,
    hint::black_box,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use tokio::{runtime::Runtime, sync::mpsc, task};
use vigil::{
    app::DiffViewMode,
    git::{
        DiffView, HighlightRegistry, build_diff_view_from_diff_text,
        build_diff_view_from_diff_text_with_context,
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
    let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benches/test.rs");
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

fn direct_viewport_highlight(mut view: DiffView, registry: &HighlightRegistry) -> usize {
    view.apply_syntax_highlighting_for_display_range(
        DiffViewMode::Split,
        SPLIT_RENDER_WIDTH,
        0,
        VIEWPORT_HEIGHT,
        FILETYPE,
        registry,
    );
    view.display_line_count(DiffViewMode::Split)
}

fn bench_app_highlight_path(c: &mut Criterion) {
    let fixture = &*GIT_RS_NEW_FILE_FIXTURE;
    let diff = &fixture.diff;
    let plain_view = build_diff_view_from_diff_text(diff, FILETYPE);
    let exact_context_view = build_diff_view_from_diff_text_with_context(
        diff,
        FILETYPE,
        None,
        Some(fixture.new_file_lines.clone()),
    );
    let registry =
        Arc::new(HighlightRegistry::new().expect("highlight registry should initialize"));
    let runtime = Runtime::new().expect("tokio runtime should initialize");

    let mut group = c.benchmark_group("app_highlight_path");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(diff.len() as u64));

    group.bench_function("clone_plain_view", |b| {
        b.iter(|| {
            let view = black_box(&plain_view).clone();
            black_box(view);
        });
    });

    group.bench_function("clone_exact_context_view", |b| {
        b.iter(|| {
            let view = black_box(&exact_context_view).clone();
            black_box(view);
        });
    });

    group.bench_function("spawn_blocking_noop", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |view| {
                let view = runtime.block_on(async move {
                    task::spawn_blocking(move || view)
                        .await
                        .expect("spawn_blocking should complete")
                });
                black_box(view);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("async_roundtrip_noop", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |view| {
                let view = runtime.block_on(async move {
                    let (sender, mut receiver) = mpsc::unbounded_channel();
                    let handle = task::spawn(async move {
                        let view = task::spawn_blocking(move || view)
                            .await
                            .expect("spawn_blocking should complete");
                        let _ = sender.send(view);
                    });
                    let view = receiver.recv().await.expect("result should arrive");
                    handle.await.expect("task should complete");
                    view
                });
                black_box(view);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("viewport_highlight_direct", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |view| {
                let line_count = direct_viewport_highlight(view, registry.as_ref());
                black_box(line_count);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("viewport_highlight_direct_with_clone", |b| {
        b.iter(|| {
            let view = plain_view.clone();
            let line_count = direct_viewport_highlight(view, registry.as_ref());
            black_box(line_count);
        });
    });

    group.bench_function("viewport_highlight_spawn_blocking", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |view| {
                let registry = Arc::clone(&registry);
                let line_count = runtime.block_on(async move {
                    task::spawn_blocking(move || direct_viewport_highlight(view, registry.as_ref()))
                        .await
                        .expect("spawn_blocking should complete")
                });
                black_box(line_count);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("viewport_highlight_spawn_blocking_with_clone", |b| {
        b.iter(|| {
            let view = plain_view.clone();
            let registry = Arc::clone(&registry);
            let line_count = runtime.block_on(async move {
                task::spawn_blocking(move || direct_viewport_highlight(view, registry.as_ref()))
                    .await
                    .expect("spawn_blocking should complete")
            });
            black_box(line_count);
        });
    });

    group.bench_function("viewport_highlight_async_roundtrip", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |view| {
                let registry = Arc::clone(&registry);
                let line_count = runtime.block_on(async move {
                    let (sender, mut receiver) = mpsc::unbounded_channel();
                    let handle = task::spawn(async move {
                        let line_count = task::spawn_blocking(move || {
                            direct_viewport_highlight(view, registry.as_ref())
                        })
                        .await
                        .expect("spawn_blocking should complete");
                        let _ = sender.send(line_count);
                    });
                    let line_count = receiver.recv().await.expect("result should arrive");
                    handle.await.expect("task should complete");
                    line_count
                });
                black_box(line_count);
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("viewport_highlight_async_roundtrip_with_clone", |b| {
        b.iter(|| {
            let view = plain_view.clone();
            let registry = Arc::clone(&registry);
            let line_count = runtime.block_on(async move {
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let handle = task::spawn(async move {
                    let line_count = task::spawn_blocking(move || {
                        direct_viewport_highlight(view, registry.as_ref())
                    })
                    .await
                    .expect("spawn_blocking should complete");
                    let _ = sender.send(line_count);
                });
                let line_count = receiver.recv().await.expect("result should arrive");
                handle.await.expect("task should complete");
                line_count
            });
            black_box(line_count);
        });
    });

    group.bench_function(
        "viewport_highlight_async_roundtrip_with_exact_context_clone",
        |b| {
            b.iter(|| {
                let view = exact_context_view.clone();
                let registry = Arc::clone(&registry);
                let line_count = runtime.block_on(async move {
                    let (sender, mut receiver) = mpsc::unbounded_channel();
                    let handle = task::spawn(async move {
                        let line_count = task::spawn_blocking(move || {
                            direct_viewport_highlight(view, registry.as_ref())
                        })
                        .await
                        .expect("spawn_blocking should complete");
                        let _ = sender.send(line_count);
                    });
                    let line_count = receiver.recv().await.expect("result should arrive");
                    handle.await.expect("task should complete");
                    line_count
                });
                black_box(line_count);
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_app_highlight_path);
criterion_main!(benches);
