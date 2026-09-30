//! Git command execution.
//!
//! This module is the adapter between Vigil's typed repository operations and
//! the `git` process. Callers should prefer product-level functions such as
//! `load_files_with_status` or `list_worktrees`; this module exists for the few
//! places that need raw command output while keeping process setup and error
//! handling in one place.
//!
//! Every command runs with `GIT_TERMINAL_PROMPT=0` and no stdin. Vigil owns
//! the terminal, so a credential prompt would corrupt the screen and wait
//! forever; git fails with a message instead.

use std::{path::Path, process::Output, time::Duration};

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
    git_command()
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
    let mut child = git_command()
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
    let mut child = git_command()
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

/// How a [`git_output_detached`] command ended.
#[derive(Debug)]
pub(crate) enum DetachedOutput {
    Finished(Output),
    /// It ran past its timeout and was terminated.
    TimedOut,
}

/// How long a timed-out command gets to exit after `SIGTERM`, which lets
/// git remove its lock files, before its process group is killed.
const TERMINATE_GRACE: Duration = Duration::from_secs(5);

/// Runs git for work nobody is watching, such as a prefetch.
///
/// Unlike [`git_output_raw`], the command runs in a session of its own, so
/// neither git nor anything it starts (ssh, credential helpers) has a
/// controlling terminal: opening `/dev/tty` to ask for a passphrase or a
/// host key confirmation fails instead of writing into vigil's screen and
/// reading its keystrokes. It also runs at most `timeout`: then its whole
/// process group gets `SIGTERM`, and `SIGKILL` if it outlives
/// [`TERMINATE_GRACE`]. Dropping the future terminates it the same way.
pub(crate) async fn git_output_detached(
    repo_root: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    timeout: Duration,
) -> color_eyre::Result<DetachedOutput> {
    let mut command = git_command();
    command
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .envs(envs.iter().copied())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    detach_from_terminal(&mut command);
    let mut child = command
        .spawn()
        .wrap_err_with(|| format!("failed to spawn git {:?}", args))?;
    let group = ProcessGroup::new(child.id());
    let stdout = read_to_end(child.stdout.take());
    let stderr = read_to_end(child.stderr.take());

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => status.wrap_err_with(|| format!("failed to wait for git {:?}", args))?,
        Err(_) => {
            group.terminate(&mut child).await;
            return Ok(DetachedOutput::TimedOut);
        }
    };
    group.disarm();
    Ok(DetachedOutput::Finished(Output {
        status,
        stdout: stdout.await.unwrap_or_default(),
        stderr: stderr.await.unwrap_or_default(),
    }))
}

fn read_to_end<R>(pipe: Option<R>) -> tokio::task::JoinHandle<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes).await;
        }
        bytes
    })
}

/// Starts the command in a new session, without a controlling terminal.
/// A new process group alone is not enough: a background process can
/// still open `/dev/tty`.
fn detach_from_terminal(command: &mut Command) {
    #[cfg(unix)]
    // SAFETY: `setsid` is async-signal-safe and touches no memory of the
    // parent, as `pre_exec` requires.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    #[cfg(not(unix))]
    let _ = command;
}

/// The process group a detached command leads. Signals the whole group, so
/// ssh and other helpers go too, and terminates it on drop unless the
/// command finished first.
struct ProcessGroup {
    leader: Option<u32>,
}

impl ProcessGroup {
    fn new(leader: Option<u32>) -> Self {
        Self { leader }
    }

    /// The command finished and was reaped; there is nothing to signal.
    fn disarm(mut self) {
        self.leader = None;
    }

    async fn terminate(mut self, child: &mut tokio::process::Child) {
        self.signal(Signal::Terminate);
        if tokio::time::timeout(TERMINATE_GRACE, child.wait())
            .await
            .is_err()
        {
            self.signal(Signal::Kill);
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        self.leader = None;
    }

    fn signal(&self, signal: Signal) {
        #[cfg(unix)]
        if let Some(leader) = self.leader.and_then(|pid| libc::pid_t::try_from(pid).ok()) {
            let signal = match signal {
                Signal::Terminate => libc::SIGTERM,
                Signal::Kill => libc::SIGKILL,
            };
            // SAFETY: plain syscall; a negative pid names the process group
            // the detached command leads, which lives until it is reaped.
            unsafe {
                libc::kill(-leader, signal);
            }
        }
        #[cfg(not(unix))]
        let _ = signal;
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.signal(Signal::Terminate);
    }
}

#[derive(Debug, Clone, Copy)]
enum Signal {
    Terminate,
    Kill,
}

fn git_command() -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null());
    command
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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Runs a shell snippet as a git alias, so it goes through the same
    /// process setup git itself gets.
    fn alias(script: &str) -> [String; 3] {
        [
            "-c".to_string(),
            format!("alias.probe=!{script}"),
            "probe".to_string(),
        ]
    }

    fn args(alias: &[String; 3]) -> Vec<&str> {
        alias.iter().map(String::as_str).collect()
    }

    async fn run(script: &str, envs: &[(&str, &str)], timeout: Duration) -> DetachedOutput {
        let probe = alias(script);
        git_output_detached(&std::env::temp_dir(), &args(&probe), envs, timeout)
            .await
            .unwrap()
    }

    /// Under a terminal (as with `script -q /dev/null cargo test`) a plain
    /// command can open `/dev/tty`; a detached one never can, which is what
    /// keeps ssh from prompting over vigil's screen.
    #[tokio::test]
    async fn a_detached_command_cannot_open_the_terminal() {
        let probe = "(: </dev/tty) 2>/dev/null && echo tty || echo detached";
        let DetachedOutput::Finished(output) = run(probe, &[], Duration::from_secs(10)).await
        else {
            panic!("the probe finishes");
        };
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "detached");
    }

    #[tokio::test]
    async fn a_detached_command_past_its_timeout_is_terminated() {
        let started = Instant::now();

        let outcome = run("sleep 30", &[], Duration::from_millis(200)).await;

        assert!(matches!(outcome, DetachedOutput::TimedOut));
        assert!(
            started.elapsed() < TERMINATE_GRACE,
            "SIGTERM stops the whole group, sleep included"
        );
    }

    #[tokio::test]
    async fn a_detached_command_passes_its_environment_and_output() {
        let script = "echo \"$VIGIL_PROBE\"; echo oops >&2; exit 3";
        let DetachedOutput::Finished(output) =
            run(script, &[("VIGIL_PROBE", "hello")], Duration::from_secs(10)).await
        else {
            panic!("the probe finishes");
        };
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "hello");
        assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "oops");
        assert_eq!(output.status.code(), Some(3));
    }
}
