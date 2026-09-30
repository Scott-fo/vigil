use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use color_eyre::Result;
use vigil::{
    app::{DiffLineWrapMode, DiffViewMode},
    forge::RepositoryRef,
    git::{
        self, BlameTarget, BranchCompareSelection, BranchMergeOutcome, BranchMergeRequest,
        CommitCompareSelection, DiffView, EMPTY_TREE_HASH, FileEntry,
    },
};

static NEXT_REPO_ID: AtomicU64 = AtomicU64::new(1);

struct TestRepo {
    root: PathBuf,
}

impl TestRepo {
    async fn init() -> Result<Self> {
        let repo_id = NEXT_REPO_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "vigil-git-integration-{}-{repo_id}",
            std::process::id()
        ));
        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }
        fs::create_dir_all(&root)?;
        git::init_repo(&root).await?;

        let repo = Self { root };
        repo.git(&["config", "user.name", "Vigil Tests"]);
        repo.git(&["config", "user.email", "vigil-tests@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        Ok(repo)
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("failed to create {}: {error}", parent.display()));
        }
        fs::write(&path, content)
            .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
    }

    fn append(&self, relative: &str, content: &str) {
        let path = self.path(relative);
        use std::io::Write;

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("failed to open {}: {error}", path.display()));
        file.write_all(content.as_bytes())
            .unwrap_or_else(|error| panic!("failed to append {}: {error}", path.display()));
    }

    fn read(&self, relative: &str) -> String {
        let path = self.path(relative);
        fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_with_env(args, &[])
    }

    fn try_git(&self, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"))
    }

    fn git_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> String {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.root).args(args);
        for (key, value) in envs {
            command.env(key, value);
        }

        let output = command
            .output()
            .unwrap_or_else(|error| panic!("failed to run git {args:?}: {error}"));

        if !output.status.success() {
            panic!(
                "git {args:?} failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn commit_all(&self, message: &str, timestamp: &str) {
        self.git(&["add", "-A"]);
        self.git_with_env(
            &["commit", "-m", message],
            &[
                ("GIT_AUTHOR_DATE", timestamp),
                ("GIT_COMMITTER_DATE", timestamp),
            ],
        );
    }

    fn rename_branch(&self, branch: &str) {
        self.git(&["branch", "-M", branch]);
    }

    fn checkout_new_branch(&self, branch: &str) {
        self.git(&["checkout", "-b", branch]);
    }

    fn checkout(&self, branch: &str) {
        self.git(&["checkout", branch]);
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn find_file(files: &[FileEntry], path: &str) -> FileEntry {
    files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("missing file entry for {path}; got {files:?}"))
        .clone()
}

fn rendered_lines(view: &mut DiffView, mode: DiffViewMode, width: usize) -> Vec<String> {
    view.rendered_lines(mode, width, DiffLineWrapMode::Wrap)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn diff_search_paths(index: &git::DiffSearchIndex, query: &str) -> Vec<String> {
    let mut matcher = git::DiffSearchMatcher::default();
    index
        .search(query, git::DiffSearchOptions::default(), &mut matcher)
        .items
        .into_iter()
        .map(|result| result.file_path)
        .collect()
}

fn selection_from_commit(file_commit: &vigil::git::CommitSearchEntry) -> CommitCompareSelection {
    CommitCompareSelection {
        base_ref: git::resolve_commit_base_ref(file_commit),
        commit_hash: file_commit.hash.clone(),
        short_hash: file_commit.short_hash.clone(),
        subject: file_commit.subject.clone(),
    }
}

#[tokio::test]
async fn worktree_listing_reports_current_branch_and_dirty_state() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("README.md", "hello\n");
    repo.commit_all("initial state", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    let linked_root = repo.root.with_file_name(format!(
        "{}-linked",
        repo.root
            .file_name()
            .expect("test repo has a file name")
            .to_string_lossy()
    ));
    if linked_root.exists() {
        let _ = fs::remove_dir_all(&linked_root);
    }
    let linked_root_arg = linked_root.to_string_lossy().to_string();
    repo.git(&[
        "worktree",
        "add",
        "-b",
        "feature/worktree",
        &linked_root_arg,
    ]);
    fs::write(linked_root.join("feature.txt"), "dirty\n")?;
    let clean_root = repo.root.with_file_name(format!(
        "{}-clean",
        repo.root
            .file_name()
            .expect("test repo has a file name")
            .to_string_lossy()
    ));
    if clean_root.exists() {
        let _ = fs::remove_dir_all(&clean_root);
    }
    let clean_root_arg = clean_root.to_string_lossy().to_string();
    repo.git(&["worktree", "add", "-b", "feature/clean", &clean_root_arg]);

    let worktrees = git::list_worktrees(&repo.root).await?;
    let canonical_root = fs::canonicalize(&repo.root)?;
    let canonical_linked_root = fs::canonicalize(&linked_root)?;
    let canonical_clean_root = fs::canonicalize(&clean_root)?;
    let current = worktrees
        .iter()
        .find(|entry| entry.path == canonical_root)
        .expect("current worktree should be listed");
    let linked = worktrees
        .iter()
        .find(|entry| entry.path == canonical_linked_root)
        .expect("linked worktree should be listed");

    assert_eq!(
        worktrees.first().map(|entry| &entry.path),
        Some(&canonical_root)
    );
    assert_eq!(current.branch.as_deref(), Some("main"));
    assert!(!current.dirty);
    assert_eq!(linked.branch.as_deref(), Some("feature/worktree"));
    assert!(linked.dirty);
    assert_eq!(linked.change_count, 1);
    assert!(
        worktrees
            .iter()
            .position(|entry| entry.path == canonical_linked_root)
            < worktrees
                .iter()
                .position(|entry| entry.path == canonical_clean_root)
    );

    repo.git(&["worktree", "remove", "--force", &linked_root_arg]);
    repo.git(&["worktree", "remove", "--force", &clean_root_arg]);
    Ok(())
}

#[tokio::test]
async fn status_stage_toggle_and_discard_cover_working_tree_flows() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write(".gitignore", "ignored.log\n");
    repo.write("src/lib.rs", "pub fn tracked() {}\n");
    repo.write("notes.md", "# original note\n");
    repo.commit_all("initial state", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.append("src/lib.rs", "pub fn changed() {}\n");
    repo.write("new/script.rs", "fn added() {}\n");
    repo.write("ignored.log", "should stay ignored\n");
    fs::create_dir_all(repo.path("docs"))?;
    repo.git(&["mv", "notes.md", "docs/notes-renamed.md"]);

    let files = git::load_files_with_status(&repo.root).await?;
    let modified = find_file(&files, "src/lib.rs");
    let untracked = find_file(&files, "new/script.rs");
    let renamed = find_file(&files, "docs/notes-renamed.md");

    assert_eq!(modified.status, " M");
    assert_eq!(modified.filetype, Some("rust"));
    assert!(!git::is_file_staged(&modified.status));
    assert_eq!(untracked.status, "??");
    assert_eq!(untracked.filetype, Some("rust"));
    assert_eq!(renamed.status, "R ");
    assert_eq!(renamed.label, "notes.md -> docs/notes-renamed.md");
    assert!(
        files.iter().all(|file| file.path != "ignored.log"),
        "ignored files should not surface in status: {files:?}"
    );

    let mut new_file_view =
        git::load_diff_view_for_working_tree(&repo.root, &untracked, None).await?;
    let rendered = rendered_lines(&mut new_file_view, DiffViewMode::Unified, 160).join("\n");
    assert!(rendered.contains("fn added() {}"));

    let search_index = git::load_diff_search_index_for_working_tree(
        &repo.root,
        &files,
        git::DiffOptions::default(),
    )
    .await?;
    assert!(diff_search_paths(&search_index, "'changed").contains(&"src/lib.rs".to_string()));
    assert!(diff_search_paths(&search_index, "'added").contains(&"new/script.rs".to_string()));

    git::stage_file(&repo.root, &modified).await?;
    let staged_status = git::load_status_for_path(&repo.root, "src/lib.rs").await?;
    assert_eq!(
        staged_status.as_ref().map(|file| file.status.as_str()),
        Some("M ")
    );
    let staged_files = git::load_files_with_status(&repo.root).await?;
    let staged = find_file(&staged_files, "src/lib.rs");
    assert_eq!(staged.status, "M ");
    assert!(git::is_file_staged(&staged.status));

    git::unstage_file(&repo.root, &staged).await?;
    let unstaged_status = git::load_status_for_path(&repo.root, "src/lib.rs").await?;
    assert_eq!(
        unstaged_status.as_ref().map(|file| file.status.as_str()),
        Some(" M")
    );
    let unstaged_files = git::load_files_with_status(&repo.root).await?;
    let unstaged = find_file(&unstaged_files, "src/lib.rs");
    assert_eq!(unstaged.status, " M");

    git::discard_file_changes(&repo.root, &unstaged).await?;
    assert!(
        git::load_status_for_path(&repo.root, "src/lib.rs")
            .await?
            .is_none()
    );
    assert_eq!(repo.read("src/lib.rs"), "pub fn tracked() {}\n");

    git::discard_file_changes(&repo.root, &untracked).await?;
    assert!(!repo.path("new/script.rs").exists());

    Ok(())
}

#[tokio::test]
async fn working_tree_diff_stats_split_tracked_and_untracked_line_counts() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("src/lib.rs", "pub fn tracked() {}\n");
    repo.commit_all("initial state", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.append("src/lib.rs", "pub fn changed() {}\n");
    repo.write("new/script.rs", "fn added() {}\nfn extra() {}\n");

    let files = git::load_files_with_status(&repo.root).await?;
    let stats = git::load_review_diff_stats_for_working_tree(
        &repo.root,
        &files,
        git::DiffOptions::default(),
    )
    .await?;
    let tracked = stats.tracked.expect("tracked scope");
    let untracked = stats.untracked.expect("untracked scope");

    assert_eq!(tracked.file_count, 1);
    assert_eq!(tracked.additions, 1);
    assert_eq!(tracked.deletions, 0);
    assert_eq!(untracked.file_count, 1);
    assert_eq!(untracked.additions, 2);
    assert_eq!(untracked.deletions, 0);
    assert_eq!(stats.file_count, 2);
    assert_eq!(stats.additions, 3);

    let snapshot = git::load_review_diff_snapshot_for_working_tree(
        &repo.root,
        &files,
        git::DiffOptions::default(),
    )
    .await?;
    let snapshot_stats = snapshot.stats_for_working_tree(&files);
    assert_eq!(snapshot_stats.tracked.map(|scope| scope.additions), Some(1));
    assert_eq!(
        snapshot_stats.untracked.map(|scope| scope.additions),
        Some(2)
    );

    Ok(())
}

#[tokio::test]
async fn ignore_whitespace_hides_reindented_lines_across_diff_loaders() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write(
        "src/lib.rs",
        "fn main() {\n    let a = 1;\n    let b = 2;\n}\n",
    );
    repo.commit_all("initial state", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    // Re-indent one line and change another for real.
    repo.write(
        "src/lib.rs",
        "fn main() {\n        let a = 1;\n    let b = 3;\n}\n",
    );
    let files = git::load_files_with_status(&repo.root).await?;
    let file = find_file(&files, "src/lib.rs");
    let ignore = git::DiffOptions {
        whitespace: git::WhitespaceMode::Ignore,
    };

    let render = |preview: &git::DiffPreviewData| -> Result<String> {
        let mut view = git::build_diff_view_from_preview_data(preview, &file, None)?;
        Ok(rendered_lines(&mut view, DiffViewMode::Unified, 120).join("\n"))
    };
    let shown = render(
        &git::load_diff_preview_for_working_tree(
            &repo.root,
            &file,
            false,
            git::DiffOptions::default(),
        )
        .await?,
    )?;
    assert!(shown.contains("-     let a = 1;"), "{shown}");
    assert!(shown.contains("-     let b = 2;"), "{shown}");

    let ignored =
        render(&git::load_diff_preview_for_working_tree(&repo.root, &file, false, ignore).await?)?;
    assert!(!ignored.contains("-     let a = 1;"), "{ignored}");
    assert!(ignored.contains("-     let b = 2;"), "{ignored}");
    assert!(ignored.contains("+     let b = 3;"), "{ignored}");

    let shown_stats = git::load_review_diff_stats_for_working_tree(
        &repo.root,
        &files,
        git::DiffOptions::default(),
    )
    .await?;
    let ignored_stats =
        git::load_review_diff_stats_for_working_tree(&repo.root, &files, ignore).await?;
    assert_eq!((shown_stats.additions, shown_stats.deletions), (2, 2));
    assert_eq!((ignored_stats.additions, ignored_stats.deletions), (1, 1));

    let snapshot =
        git::load_review_diff_snapshot_for_working_tree(&repo.root, &files, ignore).await?;
    assert_eq!(snapshot.stats().additions, 1);

    let search = git::load_diff_search_index_for_working_tree(&repo.root, &files, ignore).await?;
    assert!(diff_search_paths(&search, "'let b = 3").contains(&"src/lib.rs".to_string()));
    Ok(())
}

#[tokio::test]
async fn stage_all_changes_stages_tracked_and_untracked_files() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("src/lib.rs", "pub fn tracked() {}\n");
    repo.write("notes.md", "# notes\n");
    repo.commit_all("initial state", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.append("src/lib.rs", "pub fn changed() {}\n");
    repo.write("new/script.rs", "fn added() {}\n");
    repo.git(&["rm", "notes.md"]);

    git::stage_all_changes(&repo.root).await?;

    let files = git::load_files_with_status(&repo.root).await?;
    assert_eq!(find_file(&files, "src/lib.rs").status, "M ");
    assert_eq!(find_file(&files, "new/script.rs").status, "A ");
    assert_eq!(find_file(&files, "notes.md").status, "D ");

    Ok(())
}

#[tokio::test]
async fn unstage_all_changes_restores_working_tree_statuses() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("src/lib.rs", "pub fn tracked() {}\n");
    repo.write("notes.md", "# notes\n");
    repo.commit_all("initial state", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.append("src/lib.rs", "pub fn changed() {}\n");
    repo.write("new/script.rs", "fn added() {}\n");
    repo.git(&["rm", "notes.md"]);

    git::stage_all_changes(&repo.root).await?;
    git::unstage_all_changes(&repo.root).await?;

    let files = git::load_files_with_status(&repo.root).await?;
    assert_eq!(find_file(&files, "src/lib.rs").status, " M");
    assert_eq!(find_file(&files, "new/script.rs").status, "??");
    assert_eq!(find_file(&files, "notes.md").status, " D");
    assert!(!git::is_file_fully_staged(
        &find_file(&files, "src/lib.rs").status
    ));
    assert!(git::is_file_fully_staged("M "));

    Ok(())
}

#[tokio::test]
async fn commit_search_blame_and_commit_compare_report_expected_metadata() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("src/main.rs", "fn main() {\n    println!(\"one\");\n}\n");
    repo.commit_all("initial commit", "2024-01-02T00:00:00+0000");
    repo.rename_branch("main");

    repo.write(
        "src/main.rs",
        "fn main() {\n    println!(\"two\");\n    println!(\"three\");\n}\n",
    );
    repo.commit_all(
        "update main\n\nExpanded details for the updated file.",
        "2024-01-03T00:00:00+0000",
    );

    let commits = git::list_searchable_commits(&repo.root, 10).await?;
    assert_eq!(commits.len(), 2);
    let latest = &commits[0];
    let initial = &commits[1];
    assert_eq!(latest.subject, "update main");
    assert_eq!(latest.date, "2024-01-03");
    assert_eq!(latest.author, "Vigil Tests");
    assert_eq!(latest.parent_hashes, vec![initial.hash.clone()]);
    assert_eq!(git::resolve_commit_base_ref(latest), initial.hash);
    assert_eq!(
        git::resolve_commit_base_ref(initial),
        EMPTY_TREE_HASH.to_string()
    );

    let blame = git::load_blame_commit_details(
        &repo.root,
        &BlameTarget {
            file_path: "src/main.rs".to_string(),
            line_number: 2,
        },
    )
    .await?;
    assert!(!blame.is_uncommitted);
    assert_eq!(blame.commit_hash, latest.hash);
    assert_eq!(blame.short_hash, latest.short_hash);
    assert_eq!(blame.author, "Vigil Tests");
    assert_eq!(blame.date, "2024-01-03");
    assert_eq!(blame.subject, "update main");
    assert!(
        blame
            .description
            .contains("Expanded details for the updated file.")
    );

    let selection = selection_from_commit(latest);
    let diff_files = git::load_files_with_commit_diff(&repo.root, &selection).await?;
    let compared_file = find_file(&diff_files, "src/main.rs");
    assert_eq!(compared_file.status, "M");

    let mut diff_view =
        git::load_diff_view_for_commit_compare(&repo.root, &compared_file, &selection, None)
            .await?;
    let rendered = rendered_lines(&mut diff_view, DiffViewMode::Unified, 200).join("\n");
    assert!(rendered.contains("println!(\"two\")"));
    assert!(rendered.contains("println!(\"three\")"));

    let search_index = git::load_diff_search_index_for_commit_compare(
        &repo.root,
        &selection,
        git::DiffOptions::default(),
    )
    .await?;
    assert!(diff_search_paths(&search_index, "'three").contains(&"src/main.rs".to_string()));

    repo.write(
        "src/main.rs",
        "fn main() {\n    println!(\"two\");\n    println!(\"three\");\n    println!(\"working tree\");\n}\n",
    );
    let uncommitted = git::load_blame_commit_details(
        &repo.root,
        &BlameTarget {
            file_path: "src/main.rs".to_string(),
            line_number: 4,
        },
    )
    .await?;
    assert!(uncommitted.is_uncommitted);
    assert_eq!(uncommitted.short_hash, "working-tree");
    assert!(uncommitted.compare_selection.is_none());

    Ok(())
}

#[tokio::test]
async fn branch_compare_and_ref_listing_cover_diverged_history() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("shared.txt", "base\n");
    repo.commit_all("base", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.checkout_new_branch("feature");
    repo.write("feature.rs", "pub fn feature() {}\n");
    repo.commit_all("feature work", "2024-01-02T00:00:00+0000");

    repo.checkout("main");
    repo.write("main.txt", "main branch only\n");
    repo.commit_all("main work", "2024-01-03T00:00:00+0000");

    let refs = git::list_comparable_refs(&repo.root).await?;
    assert!(refs.iter().any(|name| name == "main"), "refs were {refs:?}");
    assert!(
        refs.iter().any(|name| name == "feature"),
        "refs were {refs:?}"
    );

    let selection = BranchCompareSelection {
        source_ref: "feature".to_string(),
        destination_ref: "main".to_string(),
    };
    let diff_files = git::load_files_with_branch_diff(&repo.root, &selection).await?;
    let feature_file = find_file(&diff_files, "feature.rs");
    assert_eq!(feature_file.status, "A");
    assert_eq!(feature_file.filetype, Some("rust"));

    let mut diff_view =
        git::load_diff_view_for_branch_compare(&repo.root, &feature_file, &selection, None).await?;
    let rendered = rendered_lines(&mut diff_view, DiffViewMode::Unified, 200).join("\n");
    assert!(rendered.contains("pub fn feature() {}"));

    let search_index = git::load_diff_search_index_for_branch_compare(
        &repo.root,
        &selection,
        git::DiffOptions::default(),
    )
    .await?;
    assert!(diff_search_paths(&search_index, "'feature").contains(&"feature.rs".to_string()));

    Ok(())
}

#[tokio::test]
async fn branch_compare_exact_highlighting_uses_merge_base_and_source_content() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write(
        "src/lib.rs",
        "pub fn shared() {\n    let value = base_call();\n}\n",
    );
    repo.commit_all("base", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    let feature_line =
        "let branch_compare_exact_rendering = feature_call_with_long_name(\"source\");";

    repo.checkout_new_branch("feature");
    repo.write(
        "src/lib.rs",
        &format!("pub fn shared() {{\n    {feature_line}\n}}\n"),
    );
    repo.commit_all("feature work", "2024-01-02T00:00:00+0000");

    repo.checkout("main");
    repo.write("src/lib.rs", "pub fn shared() {\n    x();\n}\n");
    repo.commit_all("main work", "2024-01-03T00:00:00+0000");

    let selection = BranchCompareSelection {
        source_ref: "feature".to_string(),
        destination_ref: "main".to_string(),
    };
    let diff_files = git::load_files_with_branch_diff(&repo.root, &selection).await?;
    let compared_file = find_file(&diff_files, "src/lib.rs");
    assert_eq!(compared_file.status, "M");
    assert_eq!(compared_file.filetype, Some("rust"));

    let preview = git::load_diff_preview_for_branch_compare(
        &repo.root,
        &compared_file,
        &selection,
        true,
        git::DiffOptions::default(),
    )
    .await?;
    let mut diff_view = git::build_diff_view_from_preview_data(&preview, &compared_file, None)?;
    let registry = git::HighlightRegistry::new_for_filetypes(["rust"])?;
    diff_view.apply_exact_syntax_highlighting(Some("rust"), &registry);

    let unified = rendered_lines(&mut diff_view, DiffViewMode::Unified, 200).join("\n");
    assert!(
        unified.contains(feature_line),
        "exact highlighting should preserve the feature-side text in unified mode:\n{unified}"
    );

    let split = rendered_lines(&mut diff_view, DiffViewMode::Split, 200).join("\n");
    assert!(
        split.contains(feature_line),
        "exact highlighting should preserve the feature-side text in split mode:\n{split}"
    );

    Ok(())
}

#[tokio::test]
async fn branch_merge_prepares_clean_merge_on_destination_branch() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("shared.txt", "base\n");
    repo.commit_all("base", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.checkout_new_branch("feature");
    repo.write("feature.txt", "feature work\n");
    repo.commit_all("feature work", "2024-01-02T00:00:00+0000");

    let request = BranchMergeRequest {
        source_ref: "feature".to_string(),
        destination_ref: "main".to_string(),
    };
    let outcome = git::prepare_branch_merge(&repo.root, &request).await?;

    assert_eq!(
        outcome,
        BranchMergeOutcome::Prepared {
            source_ref: "feature".to_string(),
            destination_ref: "main".to_string(),
        }
    );
    assert_eq!(repo.git(&["branch", "--show-current"]).trim(), "main");
    assert!(
        !repo
            .git(&["rev-parse", "--verify", "MERGE_HEAD"])
            .is_empty()
    );

    let status = git::load_working_tree_status(&repo.root).await?;
    let feature_file = find_file(&status.files, "feature.txt");
    assert_eq!(feature_file.status, "A ");

    Ok(())
}

#[tokio::test]
async fn branch_merge_conflicts_render_source_and_destination_names() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("shared.txt", "base\n");
    repo.commit_all("base", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.checkout_new_branch("feature");
    repo.write("shared.txt", "feature\n");
    repo.commit_all("feature work", "2024-01-02T00:00:00+0000");

    repo.checkout("main");
    repo.write("shared.txt", "main\n");
    repo.commit_all("main work", "2024-01-03T00:00:00+0000");

    let request = BranchMergeRequest {
        source_ref: "feature".to_string(),
        destination_ref: "main".to_string(),
    };
    let outcome = git::prepare_branch_merge(&repo.root, &request).await?;

    assert_eq!(
        outcome,
        BranchMergeOutcome::Conflicted {
            source_ref: "feature".to_string(),
            destination_ref: "main".to_string(),
        }
    );

    let status = git::load_working_tree_status(&repo.root).await?;
    let conflicted_file = find_file(&status.files, "shared.txt");
    assert_eq!(conflicted_file.status, "UU");

    let mut diff_view =
        git::load_diff_view_for_working_tree(&repo.root, &conflicted_file, None).await?;
    let rendered = rendered_lines(&mut diff_view, DiffViewMode::Unified, 200).join("\n");
    assert!(rendered.contains("1 Accept main"));
    assert!(rendered.contains("2 Accept feature"));
    assert!(rendered.contains("<<<<<<< HEAD (main)"));
    assert!(rendered.contains(">>>>>>> feature"));

    Ok(())
}

#[tokio::test]
async fn branch_merge_refuses_dirty_working_tree() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("shared.txt", "base\n");
    repo.commit_all("base", "2024-01-01T00:00:00+0000");
    repo.rename_branch("main");

    repo.checkout_new_branch("feature");
    repo.write("feature.txt", "feature work\n");
    repo.commit_all("feature work", "2024-01-02T00:00:00+0000");

    repo.checkout("main");
    repo.write("dirty.txt", "uncommitted\n");

    let request = BranchMergeRequest {
        source_ref: "feature".to_string(),
        destination_ref: "main".to_string(),
    };
    let error = git::prepare_branch_merge(&repo.root, &request)
        .await
        .expect_err("dirty working tree should prevent merge");

    assert!(error.to_string().contains("working tree must be clean"));
    assert!(
        !repo
            .try_git(&["rev-parse", "--verify", "MERGE_HEAD"])
            .status
            .success()
    );

    Ok(())
}

#[tokio::test]
async fn init_repo_root_resolution_commit_messages_and_empty_untracked_previews_work() -> Result<()>
{
    let repo = TestRepo::init().await?;
    fs::create_dir_all(repo.path("nested/deeper"))?;
    let resolved = git::resolve_repo_root_from(Path::new(&repo.path("nested/deeper"))).await?;
    assert_eq!(fs::canonicalize(resolved)?, fs::canonicalize(&repo.root)?);

    repo.write("tracked.txt", "base\n");
    repo.commit_all("base", "2024-01-01T00:00:00+0000");
    repo.append("tracked.txt", "next\n");
    repo.git(&["add", "tracked.txt"]);

    let error = git::commit_staged_changes(&repo.root, "   ")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Commit message is required."));

    git::commit_staged_changes(&repo.root, "  trimmed message  ").await?;
    let commits = git::list_searchable_commits(&repo.root, 5).await?;
    assert_eq!(commits[0].subject, "trimmed message");

    repo.write("empty.md", "");
    let statuses = git::load_files_with_status(&repo.root).await?;
    let empty_file = find_file(&statuses, "empty.md");
    let nested_status = git::load_working_tree_status(&repo.path("nested/deeper")).await?;
    assert_eq!(
        fs::canonicalize(&nested_status.repo_root)?,
        fs::canonicalize(&repo.root)?
    );
    assert!(
        nested_status
            .files
            .iter()
            .any(|file| file.path == "empty.md")
    );
    let mut diff_view = git::load_diff_view_for_working_tree(&repo.root, &empty_file, None).await?;
    let rendered = rendered_lines(&mut diff_view, DiffViewMode::Unified, 120).join("\n");
    assert!(rendered.contains("Untracked empty file; no textual hunk to preview."));

    Ok(())
}

#[tokio::test]
async fn refresh_path_filtering_respects_gitignore_and_special_cases() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write(".gitignore", "ignored.log\ncache/\n");
    repo.write("tracked.txt", "base\n");
    repo.commit_all("base", "2024-01-01T00:00:00+0000");

    repo.write("ignored.log", "ignored\n");
    repo.write("cache/tmp.txt", "ignored in directory\n");
    repo.append("tracked.txt", "changed\n");

    assert!(
        !git::should_refresh_for_paths(&repo.root, &[repo.path("ignored.log")]).await?,
        "ignored files should not trigger refresh"
    );
    assert!(
        !git::should_refresh_for_paths(&repo.root, &[repo.path("cache/tmp.txt")]).await?,
        "ignored directories should not trigger refresh"
    );
    assert!(git::should_refresh_for_paths(&repo.root, &[repo.path("tracked.txt")]).await?);
    assert!(git::should_refresh_for_paths(&repo.root, &[repo.root.clone()]).await?);
    assert!(git::should_refresh_for_paths(&repo.root, &[repo.path(".gitignore")]).await?);
    assert!(git::should_refresh_for_paths(&repo.root, &[]).await?);

    Ok(())
}

impl TestRepo {
    /// A bare repository to act as `origin`, and a second clone of it that
    /// plays a teammate pushing their own work.
    fn with_origin(&self) -> (TestRepo, TestRepo) {
        let remote_id = NEXT_REPO_ID.fetch_add(1, Ordering::Relaxed);
        let origin = TestRepo {
            root: self.root.with_file_name(format!(
                "vigil-git-origin-{}-{remote_id}.git",
                std::process::id()
            )),
        };
        let _ = fs::remove_dir_all(&origin.root);
        fs::create_dir_all(&origin.root).expect("create bare origin");
        origin.git(&["init", "--bare", "--initial-branch=main"]);
        self.git(&["remote", "add", "origin", origin.root.to_str().unwrap()]);

        let teammate = TestRepo {
            root: origin.root.with_extension("teammate"),
        };
        let _ = fs::remove_dir_all(&teammate.root);
        (origin, teammate)
    }

    fn clone_from(&self, origin: &TestRepo) {
        let output = Command::new("git")
            .args(["clone", "--quiet"])
            .arg(&origin.root)
            .arg(&self.root)
            .output()
            .expect("run git clone");
        assert!(output.status.success(), "git clone failed: {output:?}");
        self.git(&["config", "user.name", "Teammate"]);
        self.git(&["config", "user.email", "teammate@example.com"]);
        self.git(&["config", "commit.gpgsign", "false"]);
    }
}

async fn run_branch_op(
    repo: &TestRepo,
    operation: git::BranchOperation,
) -> std::result::Result<git::BranchOperationOutcome, git::BranchOperationError> {
    git::run_branch_operation(&repo.root, &operation).await
}

fn head_divergence(snapshot: &git::BranchSnapshot) -> Option<git::Divergence> {
    snapshot
        .head_branch()
        .and_then(|branch| branch.upstream.as_ref())
        .map(|upstream| upstream.divergence)
}

#[tokio::test]
async fn branch_operations_publish_sync_and_manage_branches() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("README.md", "hello\n");
    repo.commit_all("Initial commit", "2024-01-01T00:00:00Z");
    repo.rename_branch("main");
    let (origin, teammate) = repo.with_origin();

    // A branch without an upstream is published to origin and tracks it.
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert_eq!(snapshot.head, git::HeadState::Branch("main".to_string()));
    assert_eq!(snapshot.last_fetch, None);
    assert_eq!(head_divergence(&snapshot), None);
    assert_eq!(
        run_branch_op(&repo, git::BranchOperation::Push).await,
        Ok(git::BranchOperationOutcome::Pushed {
            upstream: "origin/main".to_string(),
            upstream_created: true,
        })
    );

    repo.append("README.md", "local\n");
    repo.commit_all("Local work", "2024-01-02T00:00:00Z");
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert_eq!(
        head_divergence(&snapshot),
        Some(git::Divergence::Tracking {
            ahead: 1,
            behind: 0
        })
    );
    run_branch_op(&repo, git::BranchOperation::Push).await?;

    // A teammate pushes to main and publishes a review branch.
    teammate.clone_from(&origin);
    teammate.write("NOTES.md", "from teammate\n");
    teammate.commit_all("Teammate notes", "2024-01-03T00:00:00Z");
    teammate.git(&["push", "--quiet", "origin", "main"]);
    teammate.checkout_new_branch("review");
    teammate.write("REVIEW.md", "please review\n");
    teammate.commit_all("Review me", "2024-01-04T00:00:00Z");
    teammate.git(&["push", "--quiet", "origin", "review"]);

    assert_eq!(
        run_branch_op(&repo, git::BranchOperation::Fetch).await,
        Ok(git::BranchOperationOutcome::Fetched)
    );
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert!(snapshot.last_fetch.is_some());
    assert_eq!(
        head_divergence(&snapshot),
        Some(git::Divergence::Tracking {
            ahead: 0,
            behind: 1
        })
    );
    let review = snapshot
        .branches
        .iter()
        .find(|branch| branch.name == "origin/review")
        .expect("fetched remote branch is listed");
    assert!(review.is_remote());
    assert_eq!(review.local_name(), "review");
    assert_eq!(review.tip.subject, "Review me");

    let pulled = run_branch_op(&repo, git::BranchOperation::Pull).await;
    assert!(matches!(
        pulled,
        Ok(git::BranchOperationOutcome::Pulled {
            up_to_date: false,
            ..
        })
    ));
    assert_eq!(repo.read("NOTES.md"), "from teammate\n");
    assert!(matches!(
        run_branch_op(&repo, git::BranchOperation::Pull).await,
        Ok(git::BranchOperationOutcome::Pulled {
            up_to_date: true,
            ..
        })
    ));

    // Checking out a remote-only branch creates a tracking local branch.
    assert_eq!(
        run_branch_op(
            &repo,
            git::BranchOperation::Track {
                remote_branch: "origin/review".to_string(),
                local_name: "review".to_string(),
            }
        )
        .await,
        Ok(git::BranchOperationOutcome::Switched {
            branch: "review".to_string()
        })
    );
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert_eq!(snapshot.previous_branch.as_deref(), Some("main"));
    assert_eq!(
        snapshot
            .head_branch()
            .and_then(|branch| branch.upstream.as_ref())
            .map(|upstream| upstream.name.as_str()),
        Some("origin/review")
    );

    // New branches never track their start point.
    run_branch_op(
        &repo,
        git::BranchOperation::Create {
            name: "spike".to_string(),
            start_point: "origin/main".to_string(),
        },
    )
    .await
    .expect("create spike");
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert_eq!(snapshot.head, git::HeadState::Branch("spike".to_string()));
    assert_eq!(snapshot.head_branch().unwrap().upstream, None);
    assert_eq!(
        run_branch_op(
            &repo,
            git::BranchOperation::Create {
                name: "bad name?".to_string(),
                start_point: "main".to_string(),
            }
        )
        .await,
        Err(git::BranchOperationError::InvalidName {
            name: "bad name?".to_string()
        })
    );

    repo.write("SPIKE.md", "unmerged\n");
    repo.commit_all("Spike", "2024-01-05T00:00:00Z");
    run_branch_op(
        &repo,
        git::BranchOperation::Rename {
            from: "spike".to_string(),
            to: "experiment".to_string(),
        },
    )
    .await
    .expect("rename spike");
    run_branch_op(
        &repo,
        git::BranchOperation::Switch {
            branch: "main".to_string(),
        },
    )
    .await
    .expect("switch to main");

    // Unmerged work is protected until the delete is forced.
    let delete = |force| git::BranchOperation::Delete {
        branch: "experiment".to_string(),
        force,
    };
    assert_eq!(
        run_branch_op(&repo, delete(false)).await,
        Err(git::BranchOperationError::NotFullyMerged {
            branch: "experiment".to_string()
        })
    );
    assert_eq!(
        run_branch_op(&repo, delete(true)).await,
        Ok(git::BranchOperationOutcome::Deleted {
            branch: "experiment".to_string()
        })
    );
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert!(snapshot.local_branch("experiment").is_none());
    assert!(snapshot.local_branch("review").is_some());
    Ok(())
}

#[tokio::test]
async fn branch_snapshot_reports_unborn_detached_and_merge_states() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.git(&["symbolic-ref", "HEAD", "refs/heads/main"]);
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert_eq!(snapshot.head, git::HeadState::Unborn("main".to_string()));
    assert!(snapshot.branches.is_empty());
    assert_eq!(
        run_branch_op(&repo, git::BranchOperation::Fetch).await,
        Err(git::BranchOperationError::NoRemote)
    );

    repo.write("app.txt", "base\n");
    repo.commit_all("Base", "2024-01-01T00:00:00Z");
    repo.checkout_new_branch("topic");
    repo.write("app.txt", "topic\n");
    repo.commit_all("Topic", "2024-01-02T00:00:00Z");
    repo.checkout("main");
    repo.write("app.txt", "main\n");
    repo.commit_all("Main", "2024-01-03T00:00:00Z");

    let merge = repo.try_git(&["merge", "topic"]);
    assert!(!merge.status.success(), "merge should conflict");
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert_eq!(snapshot.operation, Some(git::RepoOperation::Merge));
    repo.git(&["merge", "--abort"]);

    repo.git(&["checkout", "--quiet", "--detach", "HEAD"]);
    let snapshot = git::load_branch_snapshot(&repo.root).await?;
    assert!(matches!(snapshot.head, git::HeadState::Detached { .. }));
    assert_eq!(snapshot.operation, None);
    assert_eq!(
        run_branch_op(&repo, git::BranchOperation::Push).await,
        Err(git::BranchOperationError::DetachedHead)
    );
    Ok(())
}

fn acme_widgets() -> RepositoryRef {
    RepositoryRef {
        host: "github.com".to_string(),
        owner: "acme".to_string(),
        name: "widgets".to_string(),
    }
}

/// A request whose head GitHub never reported; a fetch takes whatever the
/// head is.
fn pull_request_fetch(number: u64, base_oid: &str) -> git::PullRequestFetch {
    git::PullRequestFetch {
        repository: acme_widgets(),
        number,
        head_oid: String::new(),
        base_oid: base_oid.to_string(),
        base_ref_name: "main".to_string(),
    }
}

/// A request for pull request `number` as GitHub reports it: `head` on
/// `base`.
fn reported_pull_request(number: u64, head_oid: &str, base_oid: &str) -> git::PullRequestFetch {
    git::PullRequestFetch {
        head_oid: head_oid.to_string(),
        ..pull_request_fetch(number, base_oid)
    }
}

const MISSING_OID: &str = "0123456789abcdef0123456789abcdef01234567";

#[tokio::test]
async fn pull_request_already_fetched_opens_without_the_network() -> Result<()> {
    let author = TestRepo::init().await?;
    author.write("app.txt", "base\n");
    author.commit_all("Base", "2024-01-01T00:00:00Z");
    author.rename_branch("main");
    let (origin, _teammate) = author.with_origin();
    author.git(&["push", "--quiet", "origin", "main"]);
    let base = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    author.checkout_new_branch("contribution");
    author.write("app.txt", "base\nchange\n");
    author.commit_all("Contribution", "2024-01-02T00:00:00Z");
    let head = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    author.git(&["push", "--quiet", "origin", "contribution:refs/pull/1/head"]);

    let reviewer = TestRepo::init().await?;
    reviewer.git(&["remote", "add", "origin", origin.root.to_str().unwrap()]);
    let fetched = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(1, &base))
        .await
        .expect("pull request fetches");

    // The remote goes away: any fetch now fails, so success below proves
    // the local path never needed it.
    reviewer.git(&[
        "remote",
        "set-url",
        "origin",
        "/nonexistent/vigil-origin.git",
    ]);
    let request = reported_pull_request(1, &head, &base);
    assert!(
        git::fetch_pull_request(&reviewer.root, &request)
            .await
            .is_err()
    );
    let local = git::resolve_local_pull_request(&reviewer.root, &request)
        .await
        .expect("fetched commits resolve locally");
    assert_eq!(local, fetched);
    assert_eq!(local.remote, "origin");

    // A head GitHub reports but the reviewer lacks needs a fetch, and the
    // ref keeps pointing at the commits the review had.
    let moved = reported_pull_request(1, MISSING_OID, &base);
    assert_eq!(
        git::resolve_local_pull_request(&reviewer.root, &moved).await,
        None
    );
    assert_eq!(
        reviewer.git(&["rev-parse", "refs/vigil/pr/1/head"]).trim(),
        head
    );
    Ok(())
}

#[tokio::test]
async fn pull_request_commits_already_local_get_the_review_ref_without_a_fetch() -> Result<()> {
    // The author reviews their own pull request: both commits are local,
    // but nothing fetched `refs/pull/<n>/head`.
    let author = TestRepo::init().await?;
    author.write("app.txt", "base\n");
    author.commit_all("Base", "2024-01-01T00:00:00Z");
    author.rename_branch("main");
    author.git(&["remote", "add", "origin", "/nonexistent/vigil-origin.git"]);
    let base = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    author.checkout_new_branch("feature");
    author.write("app.txt", "base\nfeature\n");
    author.commit_all("Feature", "2024-01-02T00:00:00Z");
    let head = author.git(&["rev-parse", "HEAD"]).trim().to_string();

    let local =
        git::resolve_local_pull_request(&author.root, &reported_pull_request(4, &head, &base))
            .await
            .expect("local commits resolve");
    assert_eq!(local.remote, "origin");
    assert_eq!(local.head_ref, git::pull_request_head_ref(4));
    assert_eq!(local.head_oid, head);
    assert_eq!(local.base_oid, base);
    assert_eq!(
        author.git(&["rev-parse", "refs/vigil/pr/4/head"]).trim(),
        head,
        "the head is kept reachable as a fetch would keep it"
    );

    // Missing commits, or ids too short to trust, fall through to a fetch
    // and write nothing.
    for (head_oid, base_oid) in [
        (MISSING_OID, base.as_str()),
        (head.as_str(), MISSING_OID),
        (&head[..12], base.as_str()),
        ("", base.as_str()),
    ] {
        let request = reported_pull_request(5, head_oid, base_oid);
        assert_eq!(
            git::resolve_local_pull_request(&author.root, &request).await,
            None,
            "head {head_oid:?} base {base_oid:?}"
        );
    }
    assert!(
        !author
            .try_git(&["rev-parse", "--verify", "--quiet", "refs/vigil/pr/5/head"])
            .status
            .success()
    );

    // Without a remote to name, the review could not refetch or check out,
    // so resolution defers to the fetch and its error.
    author.git(&["remote", "remove", "origin"]);
    assert_eq!(
        git::resolve_local_pull_request(&author.root, &reported_pull_request(4, &head, &base))
            .await,
        None
    );
    Ok(())
}

#[tokio::test]
async fn pull_request_fetch_brings_in_fork_heads_without_touching_branches() -> Result<()> {
    let author = TestRepo::init().await?;
    author.write("app.txt", "base\n");
    author.commit_all("Base", "2024-01-01T00:00:00Z");
    author.rename_branch("main");
    let (origin, _teammate) = author.with_origin();
    author.git(&["push", "--quiet", "origin", "main"]);
    let base = author.git(&["rev-parse", "HEAD"]).trim().to_string();

    // A fork's commits reach the base repository only as GitHub's
    // `refs/pull/<n>/head`; no branch on the remote contains them.
    author.checkout_new_branch("contribution");
    author.write("app.txt", "base\nchange\n");
    author.write("new.txt", "new\n");
    author.commit_all("Contribution", "2024-01-02T00:00:00Z");
    let head = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    author.git(&["push", "--quiet", "origin", "contribution:refs/pull/1/head"]);

    // The reviewer's `upstream` names the GitHub repository; git rewrites its
    // URL to the local bare remote, so remote selection runs on real URLs.
    // `origin` points elsewhere and must not be used.
    let reviewer = TestRepo::init().await?;
    let rewrite = format!("url.{}.insteadOf", origin.root.display());
    reviewer.git(&["config", &rewrite, "git@github.com:acme/widgets.git"]);
    reviewer.git(&["remote", "add", "origin", "/nonexistent/vigil-origin.git"]);
    reviewer.git(&[
        "remote",
        "add",
        "upstream",
        "git@github.com:acme/widgets.git",
    ]);

    let fetched = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(1, &base))
        .await
        .expect("pull request fetches");
    assert_eq!(fetched.remote, "upstream");
    assert_eq!(fetched.head_ref, git::pull_request_head_ref(1));
    assert_eq!(fetched.head_oid, head);
    assert_eq!(fetched.base_oid, base);
    assert_eq!(
        reviewer.git(&["rev-parse", "refs/vigil/pr/1/head"]).trim(),
        head
    );
    // The reviewer had no commits, so the base came from the base branch.
    reviewer.git(&["cat-file", "-e", &format!("{base}^{{commit}}")]);
    assert_eq!(
        reviewer
            .git(&[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads",
                "refs/remotes",
            ])
            .trim(),
        "",
        "no branch or remote-tracking ref is created or moved"
    );

    // The pull request diff is the branch-compare three-dot diff.
    let files = git::load_files_with_branch_diff(
        &reviewer.root,
        &BranchCompareSelection {
            source_ref: fetched.head_oid.clone(),
            destination_ref: fetched.base_oid.clone(),
        },
    )
    .await?;
    let paths = files
        .iter()
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["app.txt", "new.txt"]);

    // Refetching after a force-push moves only the vigil ref.
    author.write("new.txt", "amended\n");
    author.git(&["commit", "--quiet", "--amend", "-a", "--no-edit"]);
    let amended = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    author.git(&[
        "push",
        "--quiet",
        "--force",
        "origin",
        "contribution:refs/pull/1/head",
    ]);
    let refetched = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(1, &base))
        .await
        .expect("force-pushed pull request refetches");
    assert_eq!(refetched.head_oid, amended);

    let missing = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(2, &base)).await;
    assert!(
        matches!(
            &missing,
            Err(git::PullRequestFetchError::Fetch { remote, .. }) if remote == "upstream"
        ),
        "{missing:?}"
    );
    Ok(())
}

#[tokio::test]
async fn pull_request_base_fetch_leaves_remote_tracking_branches_alone() -> Result<()> {
    let author = TestRepo::init().await?;
    author.write("app.txt", "base\n");
    author.commit_all("Base", "2024-01-01T00:00:00Z");
    author.rename_branch("main");
    let (origin, _teammate) = author.with_origin();
    author.git(&["push", "--quiet", "origin", "main"]);

    // The reviewer tracks `origin/main` as a normal clone would.
    let reviewer = TestRepo::init().await?;
    reviewer.git(&["remote", "add", "origin", origin.root.to_str().unwrap()]);
    reviewer.git(&["fetch", "--quiet", "origin"]);
    let tracked = reviewer
        .git(&["rev-parse", "refs/remotes/origin/main"])
        .trim()
        .to_string();

    // The pull request branches off the old base, then `main` moves on: the
    // base GitHub reports is on neither the reviewer's refs nor the pull
    // request head, so the fetch has to pull `main` in.
    author.checkout_new_branch("contribution");
    author.write("new.txt", "new\n");
    author.commit_all("Contribution", "2024-01-02T00:00:00Z");
    author.git(&["push", "--quiet", "origin", "contribution:refs/pull/6/head"]);
    author.checkout("main");
    author.write("app.txt", "base\nmoved\n");
    author.commit_all("Move base", "2024-01-03T00:00:00Z");
    author.git(&["push", "--quiet", "origin", "main"]);
    let base = author.git(&["rev-parse", "HEAD"]).trim().to_string();

    let fetched = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(6, &base))
        .await
        .expect("pull request and base fetch");
    assert_eq!(fetched.base_oid, base);
    assert_eq!(
        reviewer.git(&["rev-parse", "refs/vigil/pr/6/base"]).trim(),
        base
    );
    assert_eq!(
        reviewer
            .git(&["rev-parse", "refs/remotes/origin/main"])
            .trim(),
        tracked,
        "the remote-tracking branch stays where the user's last fetch left it"
    );
    Ok(())
}

#[tokio::test]
async fn pull_request_fetch_falls_back_to_origin_and_needs_a_remote() -> Result<()> {
    let author = TestRepo::init().await?;
    author.write("app.txt", "base\n");
    author.commit_all("Base", "2024-01-01T00:00:00Z");
    author.rename_branch("main");
    let (origin, _teammate) = author.with_origin();
    author.git(&["push", "--quiet", "origin", "main"]);
    let base = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    author.git(&["push", "--quiet", "origin", "main:refs/pull/3/head"]);

    let reviewer = TestRepo::init().await?;
    assert!(matches!(
        git::fetch_pull_request(&reviewer.root, &pull_request_fetch(3, &base)).await,
        Err(git::PullRequestFetchError::NoRemote { .. })
    ));

    // A remote URL that names no GitHub repository still works as origin.
    reviewer.git(&["remote", "add", "origin", origin.root.to_str().unwrap()]);
    let fetched = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(3, &base))
        .await
        .expect("origin serves the pull request");
    assert_eq!(fetched.remote, "origin");
    assert_eq!(fetched.head_oid, base);
    Ok(())
}

#[tokio::test]
async fn pull_request_ref_cleanup_touches_only_vigil_pull_request_refs() -> Result<()> {
    let repo = TestRepo::init().await?;
    repo.write("app.txt", "base\n");
    repo.commit_all("Base", "2024-01-01T00:00:00Z");
    let head = repo.git(&["rev-parse", "HEAD"]).trim().to_string();
    for name in [
        "refs/vigil/pr/1/head",
        "refs/vigil/pr/1/base",
        "refs/vigil/pr/12/head",
        "refs/vigil/pr/2/head",
        "refs/vigil/base/main",
        "refs/vigil/base/release/1.0",
        "refs/vigil/other/keep",
        "refs/heads/feature",
        "refs/remotes/origin/pr/1",
    ] {
        repo.git(&["update-ref", name, &head]);
    }
    let refs = |repo: &TestRepo| -> Vec<String> {
        repo.git(&["for-each-ref", "--format=%(refname)"])
            .lines()
            .map(str::to_string)
            .collect()
    };

    assert_eq!(git::delete_pull_request_refs(&repo.root, 1).await?, 2);
    let after_one = refs(&repo);
    assert!(
        !after_one
            .iter()
            .any(|name| name.starts_with("refs/vigil/pr/1/"))
    );
    assert!(
        after_one.contains(&"refs/vigil/pr/12/head".to_string()),
        "pull request 12 is not pull request 1"
    );
    assert!(
        after_one.contains(&"refs/vigil/base/main".to_string()),
        "shared base branches outlive any one review"
    );
    assert_eq!(git::delete_pull_request_refs(&repo.root, 1).await?, 0);

    assert_eq!(git::prune_pull_request_refs(&repo.root).await?, 4);
    let pruned = refs(&repo);
    assert!(
        !pruned
            .iter()
            .any(|name| name.starts_with("refs/vigil/pr/") || name.starts_with("refs/vigil/base/"))
    );
    for kept in [
        "refs/vigil/other/keep",
        "refs/heads/feature",
        "refs/remotes/origin/pr/1",
    ] {
        assert!(pruned.contains(&kept.to_string()), "{kept} must survive");
    }
    Ok(())
}

/// An author with `main` pushed to a bare origin and one commit per pull
/// request number pushed as GitHub's `refs/pull/<n>/head`. Returns the
/// author, the origin, the base commit, and each pull request's head.
async fn remote_with_pull_requests(
    numbers: &[u64],
) -> Result<(TestRepo, TestRepo, String, Vec<String>)> {
    let author = TestRepo::init().await?;
    author.write("app.txt", "base\n");
    author.commit_all("Base", "2024-01-01T00:00:00Z");
    author.rename_branch("main");
    let (origin, _teammate) = author.with_origin();
    author.git(&["push", "--quiet", "origin", "main"]);
    let base = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    let mut heads = Vec::new();
    for number in numbers {
        author.checkout("main");
        author.checkout_new_branch(&format!("contribution-{number}"));
        author.write(&format!("pr-{number}.txt"), &format!("{number}\n"));
        author.commit_all(&format!("Contribution {number}"), "2024-01-02T00:00:00Z");
        heads.push(author.git(&["rev-parse", "HEAD"]).trim().to_string());
        author.git(&[
            "push",
            "--quiet",
            "origin",
            &format!("HEAD:refs/pull/{number}/head"),
        ]);
    }
    author.checkout("main");
    Ok((author, origin, base, heads))
}

fn vigil_refs(repo: &TestRepo) -> Vec<String> {
    repo.git(&["for-each-ref", "--format=%(refname)", "refs/vigil"])
        .lines()
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn pull_request_prefetch_fetches_a_batch_in_one_fetch_and_opens_locally() -> Result<()> {
    let (_author, origin, base, heads) = remote_with_pull_requests(&[1, 2, 3]).await?;
    let reviewer = TestRepo::init().await?;
    reviewer.git(&["remote", "add", "origin", origin.root.to_str().unwrap()]);
    let requests = [1, 2, 3]
        .iter()
        .zip(&heads)
        .map(|(number, head)| reported_pull_request(*number, head, &base))
        .collect::<Vec<_>>();
    for request in &requests {
        assert_eq!(
            git::resolve_local_pull_request(&reviewer.root, request).await,
            None
        );
    }

    let report = git::prefetch_pull_requests(&reviewer.root, &requests)
        .await
        .expect("prefetch runs");

    assert_eq!(report.fetches, 1, "one fetch for the whole batch");
    assert_eq!(
        report.outcomes,
        vec![
            (1, git::PrefetchOutcome::Fetched),
            (2, git::PrefetchOutcome::Fetched),
            (3, git::PrefetchOutcome::Fetched),
        ]
    );
    // Every head gets its ref; the shared base branch is fetched once, into
    // a ref of its own rather than any one pull request's.
    assert_eq!(
        vigil_refs(&reviewer),
        vec![
            "refs/vigil/base/main",
            "refs/vigil/pr/1/head",
            "refs/vigil/pr/2/head",
            "refs/vigil/pr/3/head",
        ]
    );
    assert_eq!(
        reviewer
            .git(&[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads",
                "refs/remotes",
            ])
            .trim(),
        "",
        "no branch or remote-tracking ref is created or moved"
    );

    // Opening is now local: the remote is gone and every request resolves.
    reviewer.git(&[
        "remote",
        "set-url",
        "origin",
        "/nonexistent/vigil-origin.git",
    ]);
    for (request, head) in requests.iter().zip(&heads) {
        let local = git::resolve_local_pull_request(&reviewer.root, request)
            .await
            .expect("prefetched commits resolve locally");
        assert_eq!(&local.head_oid, head);
        assert_eq!(local.base_oid, base);
    }
    Ok(())
}

#[tokio::test]
async fn pull_request_prefetch_skips_local_rows_and_survives_review_cleanup() -> Result<()> {
    let (_author, origin, base, heads) = remote_with_pull_requests(&[1, 2]).await?;
    let reviewer = TestRepo::init().await?;
    reviewer.git(&["remote", "add", "origin", origin.root.to_str().unwrap()]);
    let requests = vec![
        reported_pull_request(1, &heads[0], &base),
        reported_pull_request(2, &heads[1], &base),
    ];
    git::prefetch_pull_requests(&reviewer.root, &requests[..1])
        .await
        .expect("first prefetch runs");

    // Only the missing row is fetched: no ref of the local one is rewritten.
    let first_head_ref = reviewer.git(&["rev-parse", "refs/vigil/pr/1/head"]);
    let report = git::prefetch_pull_requests(&reviewer.root, &requests)
        .await
        .expect("second prefetch runs");
    assert_eq!(report.fetches, 1);
    assert_eq!(report.outcome(1), Some(git::PrefetchOutcome::AlreadyLocal));
    assert_eq!(report.outcome(2), Some(git::PrefetchOutcome::Fetched));
    assert_eq!(
        reviewer.git(&["rev-parse", "refs/vigil/pr/1/head"]),
        first_head_ref
    );
    assert_eq!(
        vigil_refs(&reviewer),
        vec![
            "refs/vigil/base/main",
            "refs/vigil/pr/1/head",
            "refs/vigil/pr/2/head"
        ],
        "the base branch was already fetched for #1"
    );

    // Once everything is local nothing touches the network: with the remote
    // gone the batch still succeeds, without a fetch.
    reviewer.git(&[
        "remote",
        "set-url",
        "origin",
        "/nonexistent/vigil-origin.git",
    ]);
    let report = git::prefetch_pull_requests(&reviewer.root, &requests)
        .await
        .expect("an all-local batch needs no remote access");
    assert_eq!(report.fetches, 0);
    assert_eq!(
        report.outcomes,
        vec![
            (1, git::PrefetchOutcome::AlreadyLocal),
            (2, git::PrefetchOutcome::AlreadyLocal),
        ]
    );

    // Ending a review deletes its refs, and a new session prunes them all,
    // but the commits stay: reopening resolves locally and writes the head
    // ref again.
    assert_eq!(git::delete_pull_request_refs(&reviewer.root, 1).await?, 1);
    assert!(
        vigil_refs(&reviewer).contains(&git::shared_base_ref("main")),
        "the shared base branch outlives the review"
    );
    let reopened = git::resolve_local_pull_request(&reviewer.root, &requests[0])
        .await
        .expect("reopening after the review ended is still local");
    assert_eq!(reopened.head_oid, heads[0]);
    assert_eq!(
        reviewer.git(&["rev-parse", "refs/vigil/pr/1/head"]).trim(),
        heads[0]
    );
    git::prune_pull_request_refs(&reviewer.root).await?;
    assert!(
        vigil_refs(&reviewer).is_empty(),
        "the startup prune clears all"
    );
    assert!(
        git::resolve_local_pull_request(&reviewer.root, &requests[1])
            .await
            .is_some()
    );
    Ok(())
}

/// An ssh remote that refuses the key: a prefetch reports it as an access
/// failure, which the app stops retrying, not as an ordinary fetch failure.
/// The user's own `core.sshCommand` stands in for ssh, and is used as is.
#[tokio::test]
async fn pull_request_prefetch_reports_refused_ssh_access() -> Result<()> {
    let reviewer = TestRepo::init().await?;
    reviewer.git(&[
        "remote",
        "add",
        "origin",
        "ssh://git@example.invalid/acme/widgets.git",
    ]);
    reviewer.git(&[
        "config",
        "core.sshCommand",
        "echo 'git@example.invalid: Permission denied (publickey).' >&2; exit 255; :",
    ]);
    let request = reported_pull_request(1, MISSING_OID, MISSING_OID);

    let prefetched =
        git::prefetch_pull_requests(&reviewer.root, std::slice::from_ref(&request)).await;
    assert!(
        matches!(&prefetched, Err(git::PullRequestFetchError::Access { remote, .. }) if remote == "origin"),
        "{prefetched:?}"
    );
    let fetched = git::fetch_pull_request(&reviewer.root, &request).await;
    assert!(
        matches!(fetched, Err(git::PullRequestFetchError::Access { .. })),
        "{fetched:?}"
    );
    Ok(())
}

#[tokio::test]
async fn pull_request_prefetch_fetches_a_missing_base_from_its_branch() -> Result<()> {
    let (author, origin, base, heads) = remote_with_pull_requests(&[1]).await?;
    let reviewer = TestRepo::init().await?;
    reviewer.git(&["remote", "add", "origin", origin.root.to_str().unwrap()]);
    git::prefetch_pull_requests(
        &reviewer.root,
        &[reported_pull_request(1, &heads[0], &base)],
    )
    .await
    .expect("prefetch runs");
    git::delete_pull_request_refs(&reviewer.root, 1).await?;

    // The base branch moves on; GitHub now diffs against its new tip, which
    // the reviewer lacks while the head is still local.
    author.write("app.txt", "base\nmain moved\n");
    author.commit_all("Main moves", "2024-01-03T00:00:00Z");
    author.git(&["push", "--quiet", "origin", "main"]);
    let moved_base = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    let request = reported_pull_request(1, &heads[0], &moved_base);
    assert_eq!(
        git::resolve_local_pull_request(&reviewer.root, &request).await,
        None
    );

    let report = git::prefetch_pull_requests(&reviewer.root, std::slice::from_ref(&request))
        .await
        .expect("prefetch runs");
    assert_eq!(report.fetches, 1);
    assert_eq!(report.outcome(1), Some(git::PrefetchOutcome::Fetched));
    assert_eq!(
        vigil_refs(&reviewer),
        vec!["refs/vigil/base/main"],
        "only the base branch is fetched, into its shared ref"
    );
    assert_eq!(
        reviewer
            .git(&["rev-parse", &git::shared_base_ref("main")])
            .trim(),
        moved_base
    );
    assert!(
        git::resolve_local_pull_request(&reviewer.root, &request)
            .await
            .is_some()
    );

    // A head pushed after GitHub reported it is fetched but still not the
    // reported one, so opening will fetch as usual.
    let behind = reported_pull_request(1, MISSING_OID, &moved_base);
    let report = git::prefetch_pull_requests(&reviewer.root, &[behind])
        .await
        .expect("prefetch runs");
    assert_eq!(report.outcome(1), Some(git::PrefetchOutcome::Incomplete));

    // A pull request the remote does not have fails the batch's fetch.
    let missing = reported_pull_request(9, MISSING_OID, &moved_base);
    assert!(matches!(
        git::prefetch_pull_requests(&reviewer.root, &[missing]).await,
        Err(git::PullRequestFetchError::Fetch { .. })
    ));
    Ok(())
}

fn pull_request_checkout(
    number: u64,
    branch: &str,
    head_oid: &str,
    upstream: Option<&str>,
) -> git::BranchOperation {
    git::BranchOperation::CheckoutPullRequest(git::PullRequestCheckout {
        number,
        branch: branch.to_string(),
        head_oid: head_oid.to_string(),
        upstream: upstream.map(|branch| git::RemoteBranch {
            remote: "origin".to_string(),
            branch: branch.to_string(),
        }),
    })
}

fn checked_out(
    number: u64,
    branch: &str,
    update: git::CheckoutUpdate,
) -> git::BranchOperationOutcome {
    git::BranchOperationOutcome::CheckedOutPullRequest {
        number,
        branch: branch.to_string(),
        update,
    }
}

#[tokio::test]
async fn pull_request_checkout_creates_tracks_and_fast_forwards_without_losing_work() -> Result<()>
{
    let reviewer = TestRepo::init().await?;
    reviewer.write("app.txt", "base\n");
    reviewer.commit_all("Base", "2024-01-01T00:00:00Z");
    reviewer.rename_branch("main");
    let (origin, author) = reviewer.with_origin();
    reviewer.git(&["push", "--quiet", "--set-upstream", "origin", "main"]);

    author.clone_from(&origin);
    author.checkout_new_branch("feature");
    author.write("app.txt", "base\nfeature\n");
    author.commit_all("Feature", "2024-01-02T00:00:00Z");
    author.git(&[
        "push",
        "--quiet",
        "origin",
        "feature",
        "feature:refs/pull/5/head",
    ]);
    let first = author.git(&["rev-parse", "HEAD"]).trim().to_string();
    let fetched = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(5, &first))
        .await
        .expect("pull request fetches");

    // A missing branch is created at the head and tracks the remote branch.
    assert_eq!(
        run_branch_op(
            &reviewer,
            pull_request_checkout(5, "feature", &fetched.head_oid, Some("feature"))
        )
        .await,
        Ok(checked_out(5, "feature", git::CheckoutUpdate::Created))
    );
    assert_eq!(
        reviewer.git(&["branch", "--show-current"]).trim(),
        "feature"
    );
    assert_eq!(reviewer.read("app.txt"), "base\nfeature\n");
    assert_eq!(
        reviewer
            .git(&["rev-parse", "--abbrev-ref", "feature@{upstream}"])
            .trim(),
        "origin/feature"
    );

    // Checking out again changes nothing.
    assert_eq!(
        run_branch_op(
            &reviewer,
            pull_request_checkout(5, "feature", &fetched.head_oid, Some("feature"))
        )
        .await,
        Ok(checked_out(5, "feature", git::CheckoutUpdate::UpToDate))
    );

    // New commits on the pull request fast-forward the branch, from another
    // branch or while it is checked out.
    reviewer.checkout("main");
    author.append("app.txt", "more\n");
    author.commit_all("More", "2024-01-03T00:00:00Z");
    author.git(&[
        "push",
        "--quiet",
        "origin",
        "feature",
        "feature:refs/pull/5/head",
    ]);
    let fetched = git::fetch_pull_request(&reviewer.root, &pull_request_fetch(5, &first))
        .await
        .expect("pull request refetches");
    assert_eq!(
        run_branch_op(
            &reviewer,
            pull_request_checkout(5, "feature", &fetched.head_oid, Some("feature"))
        )
        .await,
        Ok(checked_out(
            5,
            "feature",
            git::CheckoutUpdate::FastForwarded
        ))
    );
    assert_eq!(
        reviewer.git(&["rev-parse", "HEAD"]).trim(),
        fetched.head_oid
    );

    // Local commits the pull request lacks are never thrown away.
    reviewer.append("app.txt", "local\n");
    reviewer.commit_all("Local", "2024-01-04T00:00:00Z");
    let local = reviewer.git(&["rev-parse", "HEAD"]).trim().to_string();
    reviewer.checkout("main");
    assert_eq!(
        run_branch_op(
            &reviewer,
            pull_request_checkout(5, "feature", &fetched.head_oid, Some("feature"))
        )
        .await,
        Ok(checked_out(5, "feature", git::CheckoutUpdate::Diverged))
    );
    assert_eq!(reviewer.git(&["rev-parse", "HEAD"]).trim(), local);

    // A switch that would overwrite local changes is refused and moves
    // nothing.
    reviewer.checkout("main");
    reviewer.git(&["branch", "--force", "feature", &first]);
    reviewer.write("app.txt", "uncommitted\n");
    let refused = run_branch_op(
        &reviewer,
        pull_request_checkout(5, "feature", &fetched.head_oid, Some("feature")),
    )
    .await;
    assert!(
        matches!(refused, Err(git::BranchOperationError::Git { .. })),
        "{refused:?}"
    );
    assert_eq!(reviewer.git(&["branch", "--show-current"]).trim(), "main");
    assert_eq!(reviewer.git(&["rev-parse", "feature"]).trim(), first);
    assert_eq!(reviewer.read("app.txt"), "uncommitted\n");
    reviewer.git(&["checkout", "--", "app.txt"]);

    // A fork's head gets its own branch with no upstream.
    assert_eq!(
        run_branch_op(
            &reviewer,
            pull_request_checkout(5, "pr-5", &fetched.head_oid, None)
        )
        .await,
        Ok(checked_out(5, "pr-5", git::CheckoutUpdate::Created))
    );
    assert!(
        !reviewer
            .try_git(&["rev-parse", "--abbrev-ref", "pr-5@{upstream}"])
            .status
            .success()
    );
    Ok(())
}
