use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use color_eyre::Result;
use vigil::{
    app::DiffViewMode,
    git::{
        self, BlameTarget, BranchCompareSelection, CommitCompareSelection, DiffView,
        EMPTY_TREE_HASH, FileEntry,
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
    view.rendered_lines(mode, width)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
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

    git::toggle_file_stage(&repo.root, &modified).await?;
    let staged_status = git::load_status_for_path(&repo.root, "src/lib.rs").await?;
    assert_eq!(
        staged_status.as_ref().map(|file| file.status.as_str()),
        Some("M ")
    );
    let staged_files = git::load_files_with_status(&repo.root).await?;
    let staged = find_file(&staged_files, "src/lib.rs");
    assert_eq!(staged.status, "M ");
    assert!(git::is_file_staged(&staged.status));

    git::toggle_file_stage(&repo.root, &staged).await?;
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

    Ok(())
}

#[tokio::test]
async fn init_repo_root_resolution_commit_messages_and_empty_untracked_previews_work() -> Result<()>
{
    let repo = TestRepo::init().await?;
    fs::create_dir_all(repo.path("nested/deeper"))?;
    let resolved = git::resolve_repo_root_from(Path::new(&repo.path("nested/deeper"))).await?;
    assert_eq!(resolved, repo.root);

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
