//! Git command execution.
//!
//! This module is the adapter between Vigil's typed repository operations and
//! the `git` process. Callers should prefer product-level functions such as
//! `load_files_with_status` or `list_worktrees`; this module exists for the few
//! places that need raw command output while keeping process setup and error
//! handling in one place.

use std::{path::Path, process::Output};

use color_eyre::eyre::{WrapErr, eyre};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

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

pub(crate) async fn git_output_streamed<F>(
    repo_root: &Path,
    args: &[&str],
    mut on_stdout: F,
) -> color_eyre::Result<String>
where
    F: FnMut(&[u8]) -> color_eyre::Result<()>,
{
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .wrap_err_with(|| format!("failed to spawn git {:?}", args))?;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("failed to capture git {:?} stdout", args))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre!("failed to capture git {:?} stderr", args))?;

    let stderr_task = tokio::spawn(async move {
        let mut stderr_bytes = Vec::new();
        stderr
            .read_to_end(&mut stderr_bytes)
            .await
            .map(|_| stderr_bytes)
    });

    let mut stdout_bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = stdout
            .read(&mut buffer)
            .await
            .wrap_err_with(|| format!("failed to read git {:?} stdout", args))?;
        if read == 0 {
            break;
        }
        on_stdout(&buffer[..read])?;
        stdout_bytes.extend_from_slice(&buffer[..read]);
    }

    let status = child
        .wait()
        .await
        .wrap_err_with(|| format!("failed to wait for git {:?}", args))?;
    let stderr_bytes = stderr_task
        .await
        .wrap_err_with(|| format!("failed to join git {:?} stderr reader", args))?
        .wrap_err_with(|| format!("failed to read git {:?} stderr", args))?;

    if !status.success() {
        return Err(eyre!(
            "{}",
            String::from_utf8_lossy(&stderr_bytes).trim().to_string()
        ));
    }

    Ok(String::from_utf8_lossy(&stdout_bytes).into_owned())
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
