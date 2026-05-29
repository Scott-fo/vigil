use std::{fs, hint::black_box, path::PathBuf, process::Command, sync::LazyLock};

use criterion::{Criterion, criterion_group, criterion_main};
use ratatui::{Terminal, backend::TestBackend};
use tokio::runtime::Runtime;
use vigil::{
    app::App,
    git::{self, FileEntry, HighlightRegistry},
    ui,
};

struct StartupFixture {
    repo_root: PathBuf,
    selected_file: FileEntry,
    selected_filetype: &'static str,
}

static STARTUP_FIXTURE: LazyLock<StartupFixture> = LazyLock::new(build_fixture);

fn build_fixture() -> StartupFixture {
    let repo_root =
        std::env::temp_dir().join(format!("vigil-startup-bench-{}", std::process::id()));
    if repo_root.exists() {
        fs::remove_dir_all(&repo_root).expect("old bench repo should be removable");
    }
    fs::create_dir_all(repo_root.join("src/ui")).expect("bench repo dir should exist");

    run_git(&repo_root, &["init", "-q"]);
    run_git(&repo_root, &["config", "user.name", "Vigil Bench"]);
    run_git(
        &repo_root,
        &["config", "user.email", "vigil-bench@example.com"],
    );
    run_git(&repo_root, &["config", "commit.gpgsign", "false"]);

    let file_path = repo_root.join("src/ui/App.tsx");
    fs::write(
        &file_path,
        "import { memo } from 'react';\n\
type Card = { id: string; title: string; value: number };\n\
export const App = memo(({ cards }: { cards: Card[] }) => <main>{cards.map((card) => <article key={card.id}>{card.title}: {card.value}</article>)}</main>);\n",
    )
    .expect("initial bench file should be writable");
    run_git(&repo_root, &["add", "-A"]);
    run_git(&repo_root, &["commit", "-q", "-m", "initial"]);

    fs::write(
        &file_path,
        "import { memo, useMemo } from 'react';\n\
type Card = { id: string; title: string; value: number; owner: string };\n\
export const App = memo(({ cards }: { cards: Card[] }) => {\n\
  const visibleCards = useMemo(() => cards.filter((card) => card.value > 0), [cards]);\n\
  return <main>{visibleCards.map((card) => <article key={card.id}>{card.owner} / {card.title}: {card.value}</article>)}</main>;\n\
});\n",
    )
    .expect("modified bench file should be writable");

    let runtime = Runtime::new().expect("tokio runtime should initialize");
    let files = runtime
        .block_on(git::load_files_with_status(&repo_root))
        .expect("bench repo status should load");
    let selected_file = files
        .into_iter()
        .find(|file| file.path == "src/ui/App.tsx")
        .expect("bench status should include selected tsx file");
    let selected_filetype = selected_file
        .filetype
        .expect("selected bench file should have a filetype");

    StartupFixture {
        repo_root,
        selected_file,
        selected_filetype,
    }
}

fn run_git(repo_root: &PathBuf, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));

    if !output.status.success() {
        panic!(
            "git {args:?} failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn bench_startup_paths(c: &mut Criterion) {
    let fixture = &*STARTUP_FIXTURE;
    let runtime = Runtime::new().expect("tokio runtime should initialize");
    let mut group = c.benchmark_group("startup_paths");
    group.sample_size(20);

    group.bench_function("build_base_app_state", |b| {
        b.iter(|| {
            let app = App::new_for_benchmarks(black_box(fixture.repo_root.clone()));
            black_box(app);
        });
    });

    group.bench_function("render_loading_first_paint", |b| {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");
        b.iter(|| {
            let mut app = App::new_for_benchmarks(black_box(fixture.repo_root.clone()));
            app.repo_loading = true;
            app.status_message = Some("Loading repository...".to_string());
            terminal
                .draw(|frame| ui::render(frame, &mut app))
                .expect("loading first paint should render");
        });
    });

    group.bench_function("resolve_repo_root", |b| {
        b.iter(|| {
            let root = runtime
                .block_on(git::resolve_repo_root_from(black_box(&fixture.repo_root)))
                .expect("repo root should resolve");
            black_box(root);
        });
    });

    group.bench_function("load_working_tree_status", |b| {
        b.iter(|| {
            let files = runtime
                .block_on(git::load_files_with_status(black_box(&fixture.repo_root)))
                .expect("status should load");
            black_box(files);
        });
    });

    group.bench_function("load_status_with_repo_root", |b| {
        b.iter(|| {
            let status = runtime
                .block_on(git::load_working_tree_status(black_box(&fixture.repo_root)))
                .expect("status snapshot should load");
            black_box(status);
        });
    });

    group.bench_function("selected_diff_preview_plain_view", |b| {
        b.iter(|| {
            let preview = runtime
                .block_on(git::load_diff_preview_for_working_tree(
                    black_box(&fixture.repo_root),
                    black_box(&fixture.selected_file),
                    false,
                ))
                .expect("diff preview should load");
            let view =
                git::build_diff_view_from_preview_data(&preview, &fixture.selected_file, None)
                    .expect("plain diff view should build");
            black_box(view);
        });
    });

    group.bench_function("selected_highlight_registry", |b| {
        b.iter(|| {
            let registry =
                HighlightRegistry::new_for_filetypes([black_box(fixture.selected_filetype)])
                    .expect("selected highlight registry should initialize");
            black_box(registry);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_startup_paths);
criterion_main!(benches);
