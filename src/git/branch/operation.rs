use std::{fmt, path::Path, process::Output};

use super::super::{command::git_output_raw, refs::resolve_current_branch_ref};

/// A branch or remote-sync action. Pull and push act on the checked-out
/// branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchOperation {
    /// Fetches every remote and prunes deleted remote branches.
    Fetch,
    /// `git pull`, honoring the user's pull configuration.
    Pull,
    /// Pushes to the upstream. A branch without one is published to the
    /// default remote and starts tracking it.
    Push,
    Switch {
        branch: String,
    },
    /// Creates `local_name` tracking `remote_branch` and switches to it.
    Track {
        remote_branch: String,
        local_name: String,
    },
    /// Creates `name` at `start_point` and switches to it. The new branch
    /// never tracks its start point, so its first push publishes it under
    /// its own name.
    Create {
        name: String,
        start_point: String,
    },
    Rename {
        from: String,
        to: String,
    },
    /// Deletes a local branch. Without `force`, git refuses when the branch
    /// has commits that are not merged.
    Delete {
        branch: String,
        force: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchOperationOutcome {
    Fetched,
    Pulled {
        branch: String,
        up_to_date: bool,
    },
    Pushed {
        upstream: String,
        /// The push published the branch and set this upstream.
        upstream_created: bool,
    },
    Switched {
        branch: String,
    },
    Created {
        branch: String,
    },
    Renamed {
        from: String,
        to: String,
    },
    Deleted {
        branch: String,
    },
}

impl BranchOperationOutcome {
    /// Whether the checked-out files may have changed, so a review of the
    /// working tree is stale.
    pub fn changes_working_tree(&self) -> bool {
        match self {
            Self::Pulled { up_to_date, .. } => !up_to_date,
            Self::Switched { .. } | Self::Created { .. } => true,
            Self::Fetched | Self::Pushed { .. } | Self::Renamed { .. } | Self::Deleted { .. } => {
                false
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchOperationError {
    /// A safe delete was refused; deleting with `force` would lose commits.
    NotFullyMerged {
        branch: String,
    },
    InvalidName {
        name: String,
    },
    /// Pull and push need a checked-out branch.
    DetachedHead,
    NoUpstream {
        branch: String,
    },
    NoRemote,
    /// Git ran and failed; `message` is its output without hint lines.
    Git {
        message: String,
    },
}

impl fmt::Display for BranchOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFullyMerged { branch } => write!(formatter, "{branch} is not fully merged"),
            Self::InvalidName { name } if name.trim().is_empty() => {
                write!(formatter, "enter a branch name")
            }
            Self::InvalidName { name } => write!(formatter, "'{name}' is not a valid branch name"),
            Self::DetachedHead => write!(formatter, "HEAD is detached; switch to a branch first"),
            Self::NoUpstream { branch } => {
                write!(formatter, "{branch} has no upstream; push it first")
            }
            Self::NoRemote => write!(formatter, "no remote is configured"),
            Self::Git { message } => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for BranchOperationError {}

type OperationResult<T> = Result<T, BranchOperationError>;

pub async fn run_branch_operation(
    repo_root: &Path,
    operation: &BranchOperation,
) -> OperationResult<BranchOperationOutcome> {
    match operation {
        BranchOperation::Fetch => fetch(repo_root).await,
        BranchOperation::Pull => pull(repo_root).await,
        BranchOperation::Push => push(repo_root).await,
        BranchOperation::Switch { branch } => {
            run(repo_root, &["switch", branch]).await?;
            Ok(BranchOperationOutcome::Switched {
                branch: branch.clone(),
            })
        }
        BranchOperation::Track {
            remote_branch,
            local_name,
        } => {
            run(
                repo_root,
                &["switch", "--create", local_name, "--track", remote_branch],
            )
            .await
            .map_err(|error| name_error(error, local_name))?;
            Ok(BranchOperationOutcome::Switched {
                branch: local_name.clone(),
            })
        }
        BranchOperation::Create { name, start_point } => {
            let name = valid_name(name)?;
            run(
                repo_root,
                &["switch", "--no-track", "--create", name, start_point],
            )
            .await
            .map_err(|error| name_error(error, name))?;
            Ok(BranchOperationOutcome::Created {
                branch: name.to_string(),
            })
        }
        BranchOperation::Rename { from, to } => {
            let to = valid_name(to)?;
            run(repo_root, &["branch", "--move", from, to])
                .await
                .map_err(|error| name_error(error, to))?;
            Ok(BranchOperationOutcome::Renamed {
                from: from.clone(),
                to: to.to_string(),
            })
        }
        BranchOperation::Delete { branch, force } => {
            let flag = if *force { "-D" } else { "-d" };
            run(repo_root, &["branch", flag, branch])
                .await
                .map_err(|error| match error {
                    BranchOperationError::Git { message }
                        if message.contains("not fully merged") =>
                    {
                        BranchOperationError::NotFullyMerged {
                            branch: branch.clone(),
                        }
                    }
                    error => error,
                })?;
            Ok(BranchOperationOutcome::Deleted {
                branch: branch.clone(),
            })
        }
    }
}

async fn fetch(repo_root: &Path) -> OperationResult<BranchOperationOutcome> {
    if remotes(repo_root).await?.is_empty() {
        return Err(BranchOperationError::NoRemote);
    }
    run(repo_root, &["fetch", "--all", "--prune"]).await?;
    Ok(BranchOperationOutcome::Fetched)
}

async fn pull(repo_root: &Path) -> OperationResult<BranchOperationOutcome> {
    let branch = current_branch(repo_root).await?;
    if upstream(repo_root).await?.is_none() {
        return Err(BranchOperationError::NoUpstream { branch });
    }
    let output = run(repo_root, &["pull"]).await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(BranchOperationOutcome::Pulled {
        branch,
        up_to_date: stdout.contains("Already up to date") || stdout.contains("Already up-to-date"),
    })
}

async fn push(repo_root: &Path) -> OperationResult<BranchOperationOutcome> {
    let branch = current_branch(repo_root).await?;
    if let Some(upstream) = upstream(repo_root).await? {
        run(repo_root, &["push"]).await?;
        return Ok(BranchOperationOutcome::Pushed {
            upstream,
            upstream_created: false,
        });
    }

    let remote = push_remote(repo_root).await?;
    run(repo_root, &["push", "--set-upstream", &remote, &branch]).await?;
    Ok(BranchOperationOutcome::Pushed {
        upstream: format!("{remote}/{branch}"),
        upstream_created: true,
    })
}

async fn current_branch(repo_root: &Path) -> OperationResult<String> {
    resolve_current_branch_ref(repo_root)
        .await
        .map_err(|error| BranchOperationError::Git {
            message: error.to_string(),
        })?
        .ok_or(BranchOperationError::DetachedHead)
}

async fn upstream(repo_root: &Path) -> OperationResult<Option<String>> {
    let output = spawn(
        repo_root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .await?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|upstream| !upstream.is_empty()))
}

/// Remote a branch without an upstream is published to: `remote.pushDefault`,
/// then `origin`, then the first configured remote.
async fn push_remote(repo_root: &Path) -> OperationResult<String> {
    let output = spawn(repo_root, &["config", "--get", "remote.pushDefault"]).await?;
    let push_default = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !push_default.is_empty() {
        return Ok(push_default);
    }

    let remotes = remotes(repo_root).await?;
    remotes
        .iter()
        .find(|remote| *remote == "origin")
        .or_else(|| remotes.first())
        .cloned()
        .ok_or(BranchOperationError::NoRemote)
}

async fn remotes(repo_root: &Path) -> OperationResult<Vec<String>> {
    let output = run(repo_root, &["remote"]).await?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|remote| !remote.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn valid_name(name: &str) -> OperationResult<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(BranchOperationError::InvalidName {
            name: name.to_string(),
        });
    }
    Ok(trimmed)
}

fn name_error(error: BranchOperationError, name: &str) -> BranchOperationError {
    match error {
        BranchOperationError::Git { message } if message.contains("is not a valid branch name") => {
            BranchOperationError::InvalidName {
                name: name.to_string(),
            }
        }
        error => error,
    }
}

/// Runs git and requires success.
async fn run(repo_root: &Path, args: &[&str]) -> OperationResult<Output> {
    let output = spawn(repo_root, args).await?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(BranchOperationError::Git {
            message: concise_git_message(&output),
        })
    }
}

async fn spawn(repo_root: &Path, args: &[&str]) -> OperationResult<Output> {
    git_output_raw(repo_root, args)
        .await
        .map_err(|error| BranchOperationError::Git {
            message: error.to_string(),
        })
}

/// Git's failure output without `hint:` lines and `fatal:`/`error:` prefixes,
/// which read as noise in a one-line notice.
fn concise_git_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        stderr
    };
    let message = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("hint:"))
        .map(|line| {
            line.strip_prefix("fatal: ")
                .or_else(|| line.strip_prefix("error: "))
                .unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n");
    if message.is_empty() {
        "git exited without a message".to_string()
    } else {
        message
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

    use super::*;

    fn failed(stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: Vec::new(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn concise_message_drops_hints_and_severity_prefixes() {
        let output = failed(
            "error: the branch 'spike' is not fully merged\n\
             hint: If you are sure you want to delete it, run 'git branch -D spike'\n",
        );

        assert_eq!(
            concise_git_message(&output),
            "the branch 'spike' is not fully merged"
        );
    }

    #[test]
    fn only_tree_changing_outcomes_invalidate_the_review() {
        assert!(
            BranchOperationOutcome::Switched {
                branch: "main".into()
            }
            .changes_working_tree()
        );
        assert!(
            BranchOperationOutcome::Pulled {
                branch: "main".into(),
                up_to_date: false
            }
            .changes_working_tree()
        );
        assert!(
            !BranchOperationOutcome::Pulled {
                branch: "main".into(),
                up_to_date: true
            }
            .changes_working_tree()
        );
        assert!(!BranchOperationOutcome::Fetched.changes_working_tree());
    }
}
