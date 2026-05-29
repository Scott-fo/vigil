//! Git command execution.
//!
//! This module is the adapter between Vigil's typed repository operations and
//! the `git` process. Callers should prefer product-level functions such as
//! `load_files_with_status` or `list_worktrees`; this module exists for the few
//! places that need raw command output while keeping process setup and error
//! handling in one place.

use std::{path::Path, process::Output};

use color_eyre::eyre::{WrapErr, eyre};
use tokio::{io::AsyncWriteExt, process::Command};

pub async fn git_output(repo_root: &Path, args: &[&str]) -> color_eyre::Result<String> {
    let output = git_output_raw(repo_root, args).await?;
    ensure_success(&output)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) async fn git_success(repo_root: &Path, args: &[&str]) -> color_eyre::Result<()> {
    let output = git_output_raw(repo_root, args).await?;
    ensure_success(&output)
}

pub(crate) async fn git_output_bytes(
    repo_root: &Path,
    args: &[&str],
) -> color_eyre::Result<Vec<u8>> {
    let output = git_output_raw(repo_root, args).await?;
    ensure_success(&output)?;
    Ok(output.stdout)
}

pub(crate) async fn git_output_raw(repo_root: &Path, args: &[&str]) -> color_eyre::Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .await
        .wrap_err_with(|| format!("failed to run git {:?}", args))
}

pub(crate) async fn git_output_with_stdin(
    repo_root: &Path,
    args: &[&str],
    stdin: &[u8],
    accepted_codes: &[i32],
) -> color_eyre::Result<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .wrap_err_with(|| format!("failed to spawn git {:?}", args))?;

    if let Some(mut child_stdin) = child.stdin.take() {
        child_stdin
            .write_all(stdin)
            .await
            .wrap_err_with(|| format!("failed to write git {:?} stdin", args))?;
    }

    let output = child
        .wait_with_output()
        .await
        .wrap_err_with(|| format!("failed to wait for git {:?}", args))?;

    if !output
        .status
        .code()
        .is_some_and(|code| accepted_codes.contains(&code))
    {
        return Err(stderr_error(&output));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn stderr_error(output: &Output) -> color_eyre::Report {
    eyre!(
        "{}",
        String::from_utf8_lossy(&output.stderr).trim().to_string()
    )
}

fn ensure_success(output: &Output) -> color_eyre::Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(stderr_error(output))
    }
}
