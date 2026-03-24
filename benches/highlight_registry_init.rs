use criterion::{Criterion, criterion_group, criterion_main};
use vigil::git::HighlightRegistry;

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

    group.finish();
}

criterion_group!(benches, bench_highlight_registry_init);
criterion_main!(benches);
