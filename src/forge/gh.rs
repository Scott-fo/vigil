//! `gh` process execution.
//!
//! Every forge request goes through [`run_gh`]. Vigil owns the terminal, so
//! `gh` runs detached from it: stdin is null unless a request body is piped,
//! prompts, pagers, colors, and update notices are disabled, and output is
//! captured whole. Dropping the returned future kills the process.

use std::{io, path::Path, process::Stdio};

use tokio::{io::AsyncWriteExt, process::Command};

use super::error::{ForgeError, classify_failure};

const GH_ENV: &[(&str, &str)] = &[
    ("GH_PROMPT_DISABLED", "1"),
    ("GH_NO_UPDATE_NOTIFIER", "1"),
    ("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1"),
    ("GH_SPINNER_DISABLED", "1"),
    ("NO_COLOR", "1"),
    ("CLICOLOR", "0"),
    ("GH_PAGER", ""),
    ("PAGER", "cat"),
];

/// Runs `gh` in `repo_root` and returns its stdout, or a classified error.
pub(super) async fn run_gh(
    repo_root: &Path,
    args: &[String],
    stdin: Option<&[u8]>,
) -> Result<Vec<u8>, ForgeError> {
    let mut command = Command::new("gh");
    command
        .args(args)
        .current_dir(repo_root)
        .envs(GH_ENV.iter().copied())
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|error| spawn_error(args, error))?;
    if let Some(body) = stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        pipe.write_all(body)
            .await
            .map_err(|error| io_error(args, "write request body", error))?;
        // Closing the pipe signals end of input.
        drop(pipe);
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|error| io_error(args, "wait", error))?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(classify_failure(
            args,
            output.status.code(),
            &output.stdout,
            &output.stderr,
        ))
    }
}

fn spawn_error(args: &[String], error: io::Error) -> ForgeError {
    if error.kind() == io::ErrorKind::NotFound {
        ForgeError::GhNotInstalled
    } else {
        io_error(args, "start", error)
    }
}

fn io_error(args: &[String], action: &str, error: io::Error) -> ForgeError {
    ForgeError::CommandFailed {
        args: args.to_vec(),
        stderr: format!("failed to {action} gh: {error}"),
    }
}
