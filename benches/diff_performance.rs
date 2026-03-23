use std::{hint::black_box, sync::LazyLock};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use vigil::{
    app::DiffViewMode,
    git::{
        HighlightRegistry, build_diff_view_from_diff_text,
        build_diff_view_from_diff_text_with_context, clear_exact_highlight_cache,
    },
};

const FILETYPE: Option<&'static str> = Some("tsx");
const SPLIT_RENDER_WIDTH: usize = 160;
const VIEWPORT_HEIGHT: usize = 40;

struct LargeTsxFixture {
    diff: String,
    old_file_lines: Vec<String>,
    new_file_lines: Vec<String>,
}

static LARGE_TSX_FIXTURE: LazyLock<LargeTsxFixture> = LazyLock::new(build_large_tsx_fixture);

fn build_large_tsx_fixture() -> LargeTsxFixture {
    const HUNK_COUNT: usize = 12;
    const SECTIONS_PER_HUNK: usize = 24;
    const GAP_SIZE: usize = 32;

    let mut diff = String::from(
        "diff --git a/src/ui/components/mega-dashboard.tsx b/src/ui/components/mega-dashboard.tsx\n\
index 1111111..2222222 100644\n\
--- a/src/ui/components/mega-dashboard.tsx\n\
+++ b/src/ui/components/mega-dashboard.tsx\n",
    );

    let mut old_start = 1usize;
    let mut new_start = 1usize;
    let mut old_file_lines = Vec::new();
    let mut new_file_lines = Vec::new();

    for hunk_index in 0..HUNK_COUNT {
        while old_file_lines.len() + 1 < old_start {
            let line_number = old_file_lines.len() + 1;
            old_file_lines.push(format!(
                "const preservedOldLine{line_number} = `stable-old-{line_number}`;"
            ));
        }
        while new_file_lines.len() + 1 < new_start {
            let line_number = new_file_lines.len() + 1;
            new_file_lines.push(format!(
                "const preservedNewLine{line_number} = `stable-new-{line_number}`;"
            ));
        }

        let mut hunk_lines = Vec::new();
        let mut old_count = 0usize;
        let mut new_count = 0usize;

        for section_index in 0..SECTIONS_PER_HUNK {
            let global_index = hunk_index * SECTIONS_PER_HUNK + section_index;

            for line in [
                format!(
                    " import {{ memo, useEffect, useMemo, useState }} from \"react\"; // section {global_index}"
                ),
                format!(
                    " import type {{ DashboardCard, DashboardFilter, DashboardViewer }} from \"./types\"; // section {global_index}"
                ),
                format!(
                    " type DashboardSectionProps{global_index} = {{ viewer: DashboardViewer; cards: readonly DashboardCard[]; filters: readonly DashboardFilter[]; selectedId: string | null }};"
                ),
            ] {
                hunk_lines.push(format!(" {line}"));
                old_file_lines.push(line.clone());
                new_file_lines.push(line);
                old_count += 1;
                new_count += 1;
            }

            for line in [
                format!(
                    "const renderLegacyCard{global_index} = (card: DashboardCard, selectedId: string | null) => <LegacyCard key={{card.id}} title={{card.title}} subtitle={{card.subtitle}} isSelected={{selectedId === card.id}} tags={{card.tags}} metric={{card.metric}} />;"
                ),
                format!(
                    "const formatLegacyLabel{global_index} = (card: DashboardCard) => `${{card.owner}}:${{card.priority}}:${{card.environment}}:${{card.status}}`;"
                ),
                format!(
                    "const buildLegacyState{global_index} = (cards: readonly DashboardCard[]) => cards.reduce((acc, card) => acc + (card.metric > 100 ? card.metric : 0), 0);"
                ),
            ] {
                hunk_lines.push(format!("-{line}"));
                old_file_lines.push(line);
                old_count += 1;
            }

            for line in [
                format!(
                    "const renderDashboardCard{global_index} = (card: DashboardCard, selectedId: string | null, viewer: DashboardViewer) => <DashboardCardRow key={{card.id}} title={{card.title}} subtitle={{`${{card.owner}} / ${{viewer.name}} / ${{card.status}}`}} isSelected={{selectedId === card.id}} badges={{card.tags}} metric={{card.metric}} actions={{viewer.canEdit ? [\"open\", \"assign\", \"archive\"] : [\"open\"]}} />;"
                ),
                format!(
                    "const formatDashboardLabel{global_index} = (card: DashboardCard, filters: readonly DashboardFilter[]) => `${{card.owner}}:${{card.priority}}:${{card.environment}}:${{card.status}}:${{filters.map((filter) => filter.key).join(\"|\")}}`;"
                ),
                format!(
                    "const buildDashboardState{global_index} = (cards: readonly DashboardCard[], viewer: DashboardViewer) => cards.reduce((acc, card) => acc + (card.metric > 100 ? card.metric : 0) + (viewer.canEdit ? 1 : 0), 0);"
                ),
                format!(
                    "const DashboardSection{global_index} = memo(({{ viewer, cards, filters, selectedId }}: DashboardSectionProps{global_index}) => {{ const visibleCards = useMemo(() => cards.filter((card) => filters.every((filter) => filter.values.includes(String(card[filter.key as keyof DashboardCard] ?? \"\")))), [cards, filters]); return <section data-section=\"{global_index}\">{{visibleCards.map((card) => renderDashboardCard{global_index}(card, selectedId, viewer))}}</section>; }});"
                ),
            ] {
                hunk_lines.push(format!("+{line}"));
                new_file_lines.push(line);
                new_count += 1;
            }

            for line in [
                format!(
                    " export function useDashboardSection{global_index}(props: DashboardSectionProps{global_index}) {{"
                ),
                format!(
                    "   const [expanded{global_index}, setExpanded{global_index}] = useState<boolean>(props.selectedId !== null);"
                ),
                format!(
                    "   useEffect(() => {{ if (props.selectedId) setExpanded{global_index}(true); }}, [props.selectedId]);"
                ),
            ] {
                hunk_lines.push(format!(" {line}"));
                old_file_lines.push(line.clone());
                new_file_lines.push(line);
                old_count += 1;
                new_count += 1;
            }
        }

        diff.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        for line in hunk_lines {
            diff.push_str(&line);
            diff.push('\n');
        }

        old_start += old_count + GAP_SIZE;
        new_start += new_count + GAP_SIZE;
    }

    LargeTsxFixture {
        diff,
        old_file_lines,
        new_file_lines,
    }
}

fn bench_diff_pipeline(c: &mut Criterion) {
    let fixture = &*LARGE_TSX_FIXTURE;
    let diff = &fixture.diff;
    let registry = HighlightRegistry::new().expect("highlight registry should initialize");
    let plain_view = build_diff_view_from_diff_text(diff, FILETYPE);
    let exact_context_view = build_diff_view_from_diff_text_with_context(
        diff,
        FILETYPE,
        Some(fixture.old_file_lines.clone()),
        Some(fixture.new_file_lines.clone()),
    );
    let mut highlighted_view = plain_view.clone();
    highlighted_view.apply_syntax_highlighting(FILETYPE, &registry);
    let display_line_count = plain_view.clone().display_line_count(DiffViewMode::Split);
    let scrolled_viewport_start = display_line_count / 2;
    let scrolled_viewport_end = (scrolled_viewport_start + VIEWPORT_HEIGHT).min(display_line_count);

    let mut group = c.benchmark_group("diff_pipeline");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(diff.len() as u64));

    group.bench_function("build_plain_view", |b| {
        b.iter(|| {
            let view = build_diff_view_from_diff_text(black_box(diff), FILETYPE);
            black_box(view);
        });
    });

    group.bench_function("highlight_plain_view", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |mut view| {
                view.apply_syntax_highlighting(FILETYPE, &registry);
                black_box(view.display_line_count(DiffViewMode::Split));
            },
            BatchSize::LargeInput,
        );
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

    group.bench_function("render_split_plain", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |mut view| {
                let lines = view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH);
                black_box(lines.len());
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("render_split_highlighted", |b| {
        b.iter_batched(
            || highlighted_view.clone(),
            |mut view| {
                let lines = view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH);
                black_box(lines.len());
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

    group.bench_function("full_pipeline_split", |b| {
        b.iter(|| {
            let mut view = build_diff_view_from_diff_text(black_box(diff), FILETYPE);
            view.apply_syntax_highlighting(FILETYPE, &registry);
            let lines = view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH);
            black_box(lines.len());
        });
    });

    group.bench_function("exact_full_pipeline_split_warm", |b| {
        b.iter(|| {
            let mut view = build_diff_view_from_diff_text_with_context(
                black_box(diff),
                FILETYPE,
                Some(fixture.old_file_lines.clone()),
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
                Some(fixture.old_file_lines.clone()),
                Some(fixture.new_file_lines.clone()),
            );
            view.apply_exact_syntax_highlighting(FILETYPE, &registry);
            let lines = view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH);
            black_box(lines.len());
        });
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

    group.finish();
}

criterion_group!(benches, bench_diff_pipeline);
criterion_main!(benches);
