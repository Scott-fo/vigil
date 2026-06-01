use std::{hint::black_box, sync::LazyLock};

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use vigil::{
    app::{DiffLineWrapMode, DiffViewMode},
    git::{
        ChangeType, DiffHunkResolution, DiffIterationOptions, DiffStyle,
        EstimatedDiffHeightOptions, FileContents, FileDiffMetadata, HighlightRegistry, Hunk,
        HunkContent, HunkSeparatorKind, MergeConflictResolution, ParseDiffOptions,
        ProcessFileConflictData, VirtualFileMetrics, WindowFromScrollPositionOptions,
        build_diff_view_from_diff_text, build_diff_view_from_diff_text_with_context,
        clear_exact_highlight_cache, collect_diff_lines, compute_estimated_diff_heights,
        create_window_from_scroll_position, diff_accept_reject_hunk, get_merge_conflict_line_types,
        parse_diff_from_file, parse_merge_conflict_diff_from_file, parse_patch_files, process_file,
        resolve_conflict, trim_patch_context,
    },
};

const FILETYPE: Option<&'static str> = Some("tsx");
const SPLIT_RENDER_WIDTH: usize = 160;
const VIEWPORT_HEIGHT: usize = 40;
const LINE_WRAP_MODE: DiffLineWrapMode = DiffLineWrapMode::Wrap;

struct LargeTsxFixture {
    diff: String,
    old_file_lines: Vec<String>,
    new_file_lines: Vec<String>,
}

static LARGE_TSX_FIXTURE: LazyLock<LargeTsxFixture> = LazyLock::new(build_large_tsx_fixture);
static MERGE_CONFLICT_FIXTURE: LazyLock<Vec<String>> = LazyLock::new(build_merge_conflict_fixture);

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

fn build_merge_conflict_fixture() -> Vec<String> {
    let mut lines = Vec::new();
    for conflict_index in 0..600 {
        lines.push(format!("const before{conflict_index} = \"stable\";\n"));
        lines.push(format!("<<<<<<< HEAD conflict-{conflict_index}\n"));
        lines.push(format!("const value{conflict_index} = \"ours\";\n"));
        if conflict_index % 3 == 0 {
            lines.push(format!("||||||| base conflict-{conflict_index}\n"));
            lines.push(format!("const value{conflict_index} = \"base\";\n"));
        }
        lines.push("=======\n".to_string());
        lines.push(format!("const value{conflict_index} = \"theirs\";\n"));
        lines.push(format!(">>>>>>> feature-{conflict_index}\n"));
        lines.push(format!("const after{conflict_index} = \"stable\";\n"));
    }
    lines
}

fn build_merge_conflict_resolution_fixture() -> (FileDiffMetadata, ProcessFileConflictData) {
    let hunk = Hunk {
        collapsed_before: 0,
        split_line_count: 3,
        split_line_start: 0,
        unified_line_count: 3,
        unified_line_start: 0,
        addition_count: 2,
        addition_start: 1,
        addition_lines: 2,
        addition_line_index: 0,
        deletion_count: 2,
        deletion_start: 1,
        deletion_lines: 2,
        deletion_line_index: 0,
        hunk_content: vec![
            HunkContent::Change {
                deletions: 1,
                deletion_line_index: 0,
                additions: 0,
                addition_line_index: 0,
            },
            HunkContent::Context {
                lines: 1,
                addition_line_index: 0,
                deletion_line_index: 1,
            },
            HunkContent::Change {
                deletions: 0,
                deletion_line_index: 2,
                additions: 1,
                addition_line_index: 1,
            },
        ],
        hunk_context: None,
        hunk_specs: "@@ -1,2 +1,2 @@\n".to_string(),
        no_eof_cr_additions: false,
        no_eof_cr_deletions: false,
    };

    (
        FileDiffMetadata {
            name: "conflict.txt".to_string(),
            prev_name: None,
            new_object_id: None,
            prev_object_id: None,
            mode: None,
            prev_mode: None,
            change_type: ChangeType::Change,
            hunks: vec![hunk],
            split_line_count: 3,
            unified_line_count: 3,
            is_partial: false,
            deletion_lines: vec!["ours\n".to_string(), "base\n".to_string()],
            addition_lines: vec!["base\n".to_string(), "theirs\n".to_string()],
            cache_key: Some("conflict-key".to_string()),
        },
        ProcessFileConflictData {
            hunk_index: 0,
            start_content_index: 0,
            end_content_index: 2,
            current_content_index: Some(0),
            base_content_index: Some(1),
            incoming_content_index: Some(2),
            end_marker_content_index: 2,
        },
    )
}

fn bench_diff_pipeline(c: &mut Criterion) {
    let fixture = &*LARGE_TSX_FIXTURE;
    let diff = &fixture.diff;
    let merge_conflict_lines = &*MERGE_CONFLICT_FIXTURE;
    let merge_conflict_file = FileContents {
        name: "conflicts.ts".to_string(),
        contents: merge_conflict_lines.concat(),
        lang: None,
        header: None,
        cache_key: Some("conflicts".to_string()),
    };
    let (conflict_diff, conflict_data) = build_merge_conflict_resolution_fixture();
    let registry = HighlightRegistry::new().expect("highlight registry should initialize");
    let plain_view = build_diff_view_from_diff_text(diff, FILETYPE);
    let exact_context_view = build_diff_view_from_diff_text_with_context(
        diff,
        FILETYPE,
        Some(fixture.old_file_lines.clone()),
        Some(fixture.new_file_lines.clone()),
    );
    let old_file = FileContents {
        name: "mega-dashboard.tsx".to_string(),
        contents: fixture.old_file_lines.join("\n"),
        lang: None,
        header: None,
        cache_key: Some("old".to_string()),
    };
    let new_file = FileContents {
        name: "mega-dashboard.tsx".to_string(),
        contents: fixture.new_file_lines.join("\n"),
        lang: None,
        header: None,
        cache_key: Some("new".to_string()),
    };
    let full_file_diff = parse_diff_from_file(&old_file, &new_file, ParseDiffOptions::default());
    let parsed_patch_file = process_file(diff, Some("bench".to_string()), Some(true), true)
        .expect("fixture should parse")
        .expect("fixture should contain one file");
    let virtual_metrics = VirtualFileMetrics {
        hunk_line_count: 2,
        line_height: 20,
        diff_header_height: 44,
        spacing: 8,
        padding_top: None,
        padding_bottom: None,
        hunk_separator_height: None,
    };
    let mut highlighted_view = plain_view.clone();
    highlighted_view.apply_syntax_highlighting(FILETYPE, &registry);
    let display_line_count = plain_view.clone().display_line_count(
        DiffViewMode::Split,
        SPLIT_RENDER_WIDTH,
        LINE_WRAP_MODE,
    );
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

    group.bench_function("parse_patch_files", |b| {
        b.iter(|| {
            let patches = parse_patch_files(black_box(diff), Some("bench"), true)
                .expect("fixture should parse");
            black_box(patches);
        });
    });

    group.bench_function("process_file", |b| {
        b.iter(|| {
            let file = process_file(black_box(diff), Some("bench".to_string()), Some(true), true)
                .expect("fixture should parse");
            black_box(file);
        });
    });

    group.bench_function("trim_patch_context", |b| {
        b.iter(|| {
            let trimmed = trim_patch_context(black_box(diff), 10);
            black_box(trimmed);
        });
    });

    group.bench_function("diff_accept_reject_hunk", |b| {
        b.iter(|| {
            let file = diff_accept_reject_hunk(
                black_box(&parsed_patch_file),
                black_box(5),
                DiffHunkResolution::Accept,
            )
            .expect("fixture hunk should resolve");
            black_box(file);
        });
    });

    group.bench_function("compute_estimated_diff_heights", |b| {
        b.iter(|| {
            let heights = compute_estimated_diff_heights(
                black_box(&parsed_patch_file),
                EstimatedDiffHeightOptions {
                    metrics: virtual_metrics,
                    disable_file_header: false,
                    hunk_separator_kind: HunkSeparatorKind::LineInfo,
                    expand_unchanged: false,
                    expanded_hunks: None,
                    collapsed_context_threshold: 1,
                },
            )
            .expect("fixture heights should compute");
            black_box(heights);
        });
    });

    group.bench_function("create_window_from_scroll_position", |b| {
        b.iter(|| {
            let window =
                create_window_from_scroll_position(black_box(WindowFromScrollPositionOptions {
                    scroll_top: 475.25,
                    height: 100.0,
                    scroll_height: 1000.0,
                    fit_perfectly: false,
                    fit_perfectly_overscroll: 0.0,
                    overscroll_size: 30.0,
                }));
            black_box(window);
        });
    });

    group.bench_function("get_merge_conflict_line_types", |b| {
        b.iter(|| {
            let line_types = get_merge_conflict_line_types(black_box(merge_conflict_lines));
            black_box(line_types);
        });
    });

    group.bench_function("parse_merge_conflict_diff_from_file", |b| {
        b.iter(|| {
            let result = parse_merge_conflict_diff_from_file(black_box(&merge_conflict_file), 6)
                .expect("fixture merge conflicts should parse");
            black_box(result);
        });
    });

    group.bench_function("resolve_conflict", |b| {
        b.iter(|| {
            let file = resolve_conflict(
                black_box(&conflict_diff),
                black_box(&conflict_data),
                MergeConflictResolution::Incoming,
            )
            .expect("fixture conflict should resolve");
            black_box(file);
        });
    });

    group.bench_function("parse_diff_from_file", |b| {
        b.iter(|| {
            let file = parse_diff_from_file(
                black_box(&old_file),
                black_box(&new_file),
                ParseDiffOptions::default(),
            );
            black_box(file);
        });
    });

    group.bench_function("collect_diff_lines_unified", |b| {
        b.iter(|| {
            let lines = collect_diff_lines(
                black_box(&full_file_diff),
                DiffIterationOptions {
                    diff_style: DiffStyle::Unified,
                    ..DiffIterationOptions::default()
                },
            )
            .expect("full-file diff should iterate");
            black_box(lines);
        });
    });

    group.bench_function("collect_diff_lines_split", |b| {
        b.iter(|| {
            let lines = collect_diff_lines(
                black_box(&full_file_diff),
                DiffIterationOptions {
                    diff_style: DiffStyle::Split,
                    ..DiffIterationOptions::default()
                },
            )
            .expect("full-file diff should iterate");
            black_box(lines);
        });
    });

    group.bench_function("highlight_plain_view", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |mut view| {
                view.apply_syntax_highlighting(FILETYPE, &registry);
                black_box(view.display_line_count(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    LINE_WRAP_MODE,
                ));
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
                    LINE_WRAP_MODE,
                    0,
                    VIEWPORT_HEIGHT,
                    FILETYPE,
                    &registry,
                );
                black_box(view.display_line_count(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    LINE_WRAP_MODE,
                ));
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
                    LINE_WRAP_MODE,
                    scrolled_viewport_start,
                    scrolled_viewport_end,
                    FILETYPE,
                    &registry,
                );
                black_box(view.display_line_count(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    LINE_WRAP_MODE,
                ));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("render_split_plain", |b| {
        b.iter_batched(
            || plain_view.clone(),
            |mut view| {
                let lines =
                    view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH, LINE_WRAP_MODE);
                black_box(lines.len());
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("render_split_highlighted", |b| {
        b.iter_batched(
            || highlighted_view.clone(),
            |mut view| {
                let lines =
                    view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH, LINE_WRAP_MODE);
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
                black_box(view.display_line_count(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    LINE_WRAP_MODE,
                ));
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
                black_box(view.display_line_count(
                    DiffViewMode::Split,
                    SPLIT_RENDER_WIDTH,
                    LINE_WRAP_MODE,
                ));
            },
            BatchSize::LargeInput,
        );
    });

    group.bench_function("full_pipeline_split", |b| {
        b.iter(|| {
            let mut view = build_diff_view_from_diff_text(black_box(diff), FILETYPE);
            view.apply_syntax_highlighting(FILETYPE, &registry);
            let lines =
                view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH, LINE_WRAP_MODE);
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
            let lines =
                view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH, LINE_WRAP_MODE);
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
            let lines =
                view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH, LINE_WRAP_MODE);
            black_box(lines.len());
        });
    });

    group.bench_function("initial_viewport_pipeline_split", |b| {
        b.iter(|| {
            let mut view = build_diff_view_from_diff_text(black_box(diff), FILETYPE);
            view.apply_syntax_highlighting_for_display_range(
                DiffViewMode::Split,
                SPLIT_RENDER_WIDTH,
                LINE_WRAP_MODE,
                0,
                VIEWPORT_HEIGHT,
                FILETYPE,
                &registry,
            );
            let lines =
                view.rendered_lines(DiffViewMode::Split, SPLIT_RENDER_WIDTH, LINE_WRAP_MODE);
            black_box(lines.len());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_diff_pipeline);
criterion_main!(benches);
