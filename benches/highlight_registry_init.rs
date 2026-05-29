use criterion::{Criterion, criterion_group, criterion_main};
use vigil::git::{HighlightRegistry, prewarm_highlight_registry};

const SELECTED_TSX: [&str; 1] = ["tsx"];
const NEARBY_FILETYPES: [&str; 4] = ["typescript", "rust", "json", "go"];
const MANY_CHANGED_FILETYPES: [&str; 21] = [
    "rust",
    "javascript",
    "jsx",
    "typescript",
    "python",
    "go",
    "c",
    "cpp",
    "csharp",
    "bash",
    "java",
    "ruby",
    "php",
    "scala",
    "html",
    "json",
    "yaml",
    "haskell",
    "css",
    "nix",
    "zig",
];

fn bench_highlight_registry_init(c: &mut Criterion) {
    let mut group = c.benchmark_group("highlight_registry_init");
    group.sample_size(10);

    group.bench_function("new_selected_rust_only", |b| {
        b.iter(|| {
            let registry = HighlightRegistry::new_for_filetypes(["rust"])
                .expect("selected-filetype registry should initialize");
            criterion::black_box(registry);
        });
    });

    group.bench_function("new_selected_tsx_only", |b| {
        b.iter(|| {
            let registry = HighlightRegistry::new_for_filetypes(["tsx"])
                .expect("selected-filetype registry should initialize");
            criterion::black_box(registry);
        });
    });

    group.bench_function("new_full_registry", |b| {
        b.iter(|| {
            let registry =
                HighlightRegistry::new().expect("full highlight registry should initialize");
            criterion::black_box(registry);
        });
    });

    group.bench_function("prewarm_nearby_after_selected_tsx", |b| {
        b.iter(|| {
            let registry = HighlightRegistry::new_for_filetypes(SELECTED_TSX)
                .expect("selected-filetype registry should initialize");
            prewarm_highlight_registry(&registry, NEARBY_FILETYPES)
                .expect("nearby filetypes should prewarm");
            criterion::black_box(registry);
        });
    });

    group.bench_function("prewarm_many_changed_after_selected_tsx", |b| {
        b.iter(|| {
            let registry = HighlightRegistry::new_for_filetypes(SELECTED_TSX)
                .expect("selected-filetype registry should initialize");
            prewarm_highlight_registry(&registry, MANY_CHANGED_FILETYPES)
                .expect("changed filetypes should prewarm");
            criterion::black_box(registry);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_highlight_registry_init);
criterion_main!(benches);
