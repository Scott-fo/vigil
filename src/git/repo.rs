use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
};

use color_eyre::eyre::{WrapErr, eyre};
use tokio::{fs, process::Command};

use crate::theme;

use super::{
    BlameCommitDetails, BlameTarget, BranchCompareSelection, CommitCompareSelection,
    CommitSearchEntry, EMPTY_TREE_HASH, FileEntry,
    parse::{
        build_branch_diff_range, is_uncommitted_blame_hash, parse_blame_porcelain_header,
        parse_commit_log_entries, parse_commit_show_output, parse_diff_name_status_entries,
        parse_status_entries, to_file_entry,
    },
};

pub fn is_file_staged(status: &str) -> bool {
    if status == "??" {
        return false;
    }

    let index_status = status.chars().next().unwrap_or(' ');
    index_status != ' '
}

pub fn is_file_fully_staged(status: &str) -> bool {
    if status == "??" {
        return false;
    }

    let mut chars = status.chars();
    let index_status = chars.next().unwrap_or(' ');
    let worktree_status = chars.next().unwrap_or(' ');
    index_status != ' ' && worktree_status == ' '
}

pub fn status_color(status: &str) -> ratatui::style::Color {
    let palette = theme::active_palette();
    if status == "??" {
        return palette.success;
    }
    if status.contains('D') {
        return palette.error;
    }
    if status.contains('R') || status.contains('C') {
        return palette.secondary;
    }
    if status.contains('M') {
        return palette.warning;
    }
    palette.text_muted
}

pub async fn toggle_file_stage(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    if is_file_staged(&file.status) {
        run_git_action(
            repo_root,
            &["restore", "--staged", "--", file.path.as_str()],
        )
        .await
    } else {
        run_git_action(repo_root, &["add", "--", file.path.as_str()]).await
    }
}

pub async fn stage_all_changes(repo_root: &Path) -> color_eyre::Result<()> {
    run_git_action(repo_root, &["add", "-A"]).await
}

pub async fn unstage_all_changes(repo_root: &Path) -> color_eyre::Result<()> {
    run_git_action(repo_root, &["restore", "--staged", "--", "."]).await
}

pub async fn discard_file_changes(repo_root: &Path, file: &FileEntry) -> color_eyre::Result<()> {
    if file.status == "??" {
        run_git_action(repo_root, &["clean", "-f", "--", file.path.as_str()]).await
    } else {
        run_git_action(
            repo_root,
            &[
                "restore",
                "--source=HEAD",
                "--staged",
                "--worktree",
                "--",
                file.path.as_str(),
            ],
        )
        .await
    }
}

pub async fn commit_staged_changes(repo_root: &Path, message: &str) -> color_eyre::Result<()> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(eyre!("Commit message is required."));
    }

    run_git_action(repo_root, &["commit", "-m", trimmed]).await
}

pub async fn push_to_remote(repo_root: &Path) -> color_eyre::Result<()> {
    run_git_action(repo_root, &["push"]).await
}

pub async fn pull_from_remote(repo_root: &Path) -> color_eyre::Result<()> {
    run_git_action(repo_root, &["pull"]).await
}

pub async fn init_repo(repo_root: &Path) -> color_eyre::Result<()> {
    run_git_action(repo_root, &["init"]).await
}

pub async fn list_searchable_commits(
    repo_root: &Path,
    limit: usize,
) -> color_eyre::Result<Vec<CommitSearchEntry>> {
    let output = git_output(
        repo_root,
        &[
            "log",
            &format!("--max-count={}", limit.max(1)),
            "--date=short",
            "--pretty=format:%H%x1f%P%x1f%h%x1f%ad%x1f%an%x1f%s%x1e",
        ],
    )
    .await?;

    Ok(parse_commit_log_entries(&output))
}

pub fn resolve_commit_base_ref(commit: &CommitSearchEntry) -> String {
    commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string())
}

pub async fn load_blame_commit_details(
    repo_root: &Path,
    target: &BlameTarget,
) -> color_eyre::Result<BlameCommitDetails> {
    let blame_output = git_output(
        repo_root,
        &[
            "blame",
            "--porcelain",
            "-L",
            &format!("{0},{0}", target.line_number),
            "--",
            target.file_path.as_str(),
        ],
    )
    .await?;

    let header = parse_blame_porcelain_header(&blame_output).ok_or_else(|| {
        eyre!(
            "unable to parse blame output for {}:{}",
            target.file_path,
            target.line_number
        )
    })?;

    if is_uncommitted_blame_hash(&header.commit_hash) {
        return Ok(BlameCommitDetails {
            target: target.clone(),
            commit_hash: header.commit_hash,
            short_hash: "working-tree".to_string(),
            author: if header.author.is_empty() {
                "Uncommitted".to_string()
            } else {
                header.author
            },
            date: header.date,
            subject: if header.summary.is_empty() {
                "Uncommitted line changes".to_string()
            } else {
                header.summary
            },
            description: "This line has uncommitted changes. Commit comparison is unavailable."
                .to_string(),
            is_uncommitted: true,
            compare_selection: None,
        });
    }

    let show_output = git_output(
        repo_root,
        &[
            "show",
            "-s",
            "--date=short",
            "--format=%H%x1f%h%x1f%P%x1f%ad%x1f%an%x1f%s%x1f%b",
            header.commit_hash.as_str(),
        ],
    )
    .await?;

    let commit = parse_commit_show_output(&show_output)
        .ok_or_else(|| eyre!("unable to parse commit metadata for {}", header.commit_hash))?;
    let subject = if commit.subject.is_empty() {
        header.summary
    } else {
        commit.subject
    };
    let description = if commit.description.trim().is_empty() {
        "No commit description.".to_string()
    } else {
        commit.description
    };
    let compare_base = commit
        .parent_hashes
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_HASH.to_string());
    let commit_hash = commit.commit_hash;
    let short_hash = commit.short_hash;

    Ok(BlameCommitDetails {
        target: target.clone(),
        commit_hash: commit_hash.clone(),
        short_hash: short_hash.clone(),
        author: if commit.author.is_empty() {
            header.author
        } else {
            commit.author
        },
        date: if commit.date.is_empty() {
            header.date
        } else {
            commit.date
        },
        description,
        is_uncommitted: false,
        compare_selection: Some(CommitCompareSelection {
            base_ref: compare_base,
            commit_hash: commit_hash.clone(),
            short_hash: short_hash.clone(),
            subject: subject.clone(),
        }),
        subject,
    })
}

pub async fn load_files_with_commit_diff(
    repo_root: &Path,
    selection: &CommitCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    load_diff_name_status_files(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            selection.base_ref.as_str(),
            selection.commit_hash.as_str(),
        ],
    )
    .await
}

pub async fn list_comparable_refs(repo_root: &Path) -> color_eyre::Result<Vec<String>> {
    let output = git_output(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)\t%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .await?;

    let mut refs = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (full_ref, short_ref) = line.split_once('\t')?;
            let short_ref = short_ref.trim();
            if short_ref.is_empty() || short_ref == "HEAD" {
                return None;
            }
            if full_ref.starts_with("refs/remotes/")
                && (!short_ref.contains('/') || short_ref.ends_with("/HEAD"))
            {
                return None;
            }
            Some(short_ref.to_string())
        })
        .collect::<Vec<_>>();

    refs.sort();
    refs.dedup();
    Ok(refs)
}

pub async fn load_files_with_branch_diff(
    repo_root: &Path,
    selection: &BranchCompareSelection,
) -> color_eyre::Result<Vec<FileEntry>> {
    load_diff_name_status_files(
        repo_root,
        &[
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            build_branch_diff_range(selection).as_str(),
        ],
    )
    .await
}

pub async fn should_refresh_for_paths(
    repo_root: &Path,
    changed_paths: &[PathBuf],
) -> color_eyre::Result<bool> {
    if changed_paths.is_empty() {
        return Ok(true);
    }

    let mut candidate_paths = Vec::new();
    let mut seen_paths = HashSet::new();

    for path in changed_paths {
        let Ok(relative_path) = path.strip_prefix(repo_root) else {
            return Ok(true);
        };

        if relative_path.as_os_str().is_empty() {
            return Ok(true);
        }

        if relative_path
            .file_name()
            .is_some_and(|file_name| file_name == ".gitignore")
        {
            return Ok(true);
        }

        let relative = relative_path.to_string_lossy().replace('\\', "/");
        if seen_paths.insert(relative.clone()) {
            candidate_paths.push(relative);
        }
    }

    if candidate_paths.is_empty() {
        return Ok(false);
    }

    let ignored_paths = git_check_ignored(repo_root, &candidate_paths).await?;
    Ok(candidate_paths
        .iter()
        .any(|path| !ignored_paths.contains(path)))
}

pub async fn resolve_repo_root() -> color_eyre::Result<PathBuf> {
    resolve_repo_root_from(Path::new(".")).await
}

pub async fn resolve_repo_root_from(probe_path: &Path) -> color_eyre::Result<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(probe_path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await
        .wrap_err("failed to resolve git repository root")?;

    if !output.status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }

    Ok(PathBuf::from(
        String::from_utf8_lossy(&output.stdout).trim(),
    ))
}

pub async fn load_files_with_status(repo_root: &Path) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(
        repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    let mut files = Vec::new();

    for entry in parse_status_entries(&output) {
        if entry.status == "!!" || is_directory_status_entry(repo_root, &entry.path).await {
            continue;
        }
        files.push(to_file_entry(entry));
    }

    Ok(files)
}

pub async fn load_status_for_path(
    repo_root: &Path,
    file_path: &str,
) -> color_eyre::Result<Option<FileEntry>> {
    let output = git_output(
        repo_root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            file_path,
        ],
    )
    .await?;

    for entry in parse_status_entries(&output) {
        if entry.status == "!!" || is_directory_status_entry(repo_root, &entry.path).await {
            continue;
        }
        return Ok(Some(to_file_entry(entry)));
    }

    Ok(None)
}

async fn is_directory_status_entry(repo_root: &Path, path: &str) -> bool {
    match fs::metadata(repo_root.join(path)).await {
        Ok(metadata) => metadata.is_dir(),
        Err(_) => false,
    }
}

async fn run_git_action(repo_root: &Path, args: &[&str]) -> color_eyre::Result<()> {
    let _ = git_output(repo_root, args).await?;
    Ok(())
}

async fn load_diff_name_status_files(
    repo_root: &Path,
    args: &[&str],
) -> color_eyre::Result<Vec<FileEntry>> {
    let output = git_output(repo_root, args).await?;
    Ok(parse_diff_name_status_entries(&output)
        .into_iter()
        .map(to_file_entry)
        .collect())
}

pub async fn git_output(repo_root: &Path, args: &[&str]) -> color_eyre::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .await
        .wrap_err_with(|| format!("failed to run git {:?}", args))?;

    if !output.status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn git_check_ignored(
    repo_root: &Path,
    paths: &[String],
) -> color_eyre::Result<HashSet<String>> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_root);
    command.args(["check-ignore", "-z", "--stdin"]);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .wrap_err("failed to spawn git check-ignore")?;

    if let Some(mut stdin) = child.stdin.take() {
        let input = format!("{}\0", paths.join("\0"));
        tokio::io::AsyncWriteExt::write_all(&mut stdin, input.as_bytes())
            .await
            .wrap_err("failed to write git check-ignore stdin")?;
    }

    let output = child
        .wait_with_output()
        .await
        .wrap_err("failed to wait for git check-ignore")?;

    match output.status.code() {
        Some(0) | Some(1) => {}
        _ => {
            return Err(eyre!(
                "{}",
                String::from_utf8_lossy(&output.stderr).trim().to_string()
            ));
        }
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}
