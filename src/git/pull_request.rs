//! Pull request commits in the local object store.
//!
//! Reviewing a pull request locally needs its head and base commits, which a
//! plain `git fetch` does not bring in when the head lives in a fork.
//! [`fetch_pull_request`] fetches GitHub's `refs/pull/<n>/head` into a
//! vigil-private ref ([`pull_request_head_ref`]) so the commits stay reachable
//! (and safe from `git gc`) while they are reviewed, then makes sure the base
//! commit is present too.
//!
//! The fetch never checks anything out and never moves user branches or
//! remote-tracking refs; it only writes refs under `refs/vigil/pr/`. It runs
//! against the remote whose URL points at the pull request's repository
//! (ssh and https forms both match), falling back to `origin`. Like every git
//! command here it runs with `GIT_TERMINAL_PROMPT=0`, so a remote that needs
//! credentials fails instead of prompting.
//!
//! This is network I/O: seconds on a slow link, and over a second even when
//! nothing new arrives. Interactive callers should run it in the background
//! and treat the result as a snapshot of the pull request at fetch time.
//!
//! [`resolve_local_pull_request`] is the local-only counterpart: when the
//! head and base commits GitHub last reported are already in the object store
//! (a previous fetch, a prefetch, or the author's own branch), it points the
//! same vigil-private ref at the head and returns what a fetch would have,
//! in a few milliseconds and without touching the network. Callers try it
//! first and fetch only when it returns `None`.

use std::{fmt, path::Path};

use super::{
    command::{git_output, git_output_raw, git_output_with_stdin},
    remote::{Remote, list_remotes, parse_remote_url},
};
use crate::forge::RepositoryRef;

/// What to fetch for one pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestFetch {
    /// The repository that owns the pull request, used to pick a remote.
    pub repository: RepositoryRef,
    pub number: u64,
    /// The head commit GitHub last reported. A fetch takes whatever the head
    /// is now; [`resolve_local_pull_request`] accepts only this commit.
    pub head_oid: String,
    /// The base commit GitHub computed the diff against.
    pub base_oid: String,
    /// The base branch name, fetched when `base_oid` is missing locally.
    pub base_ref_name: String,
}

/// The pull request as it now exists locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedPullRequest {
    /// The remote the commits came from.
    pub remote: String,
    /// The vigil-private ref pointing at the fetched head.
    pub head_ref: String,
    /// The head commit as fetched. May be newer than the head a caller last
    /// saw on GitHub if the branch was pushed in between.
    pub head_oid: String,
    /// The base commit, verified to be present locally.
    pub base_oid: String,
}

/// Why a pull request could not be fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRequestFetchError {
    /// No remote matches the repository and there is no `origin`.
    NoRemote { repository: String },
    /// `git fetch` failed: offline, no access, or the pull request ref does
    /// not exist on the remote.
    Fetch { remote: String, stderr: String },
    /// The base commit is still missing after fetching the base branch.
    MissingBase { oid: String },
    /// Another git command failed.
    Git { message: String },
}

impl fmt::Display for PullRequestFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRemote { repository } => {
                write!(formatter, "no git remote points at {repository}")
            }
            Self::Fetch { remote, stderr } => {
                write!(formatter, "git fetch from {remote} failed: {stderr}")
            }
            Self::MissingBase { oid } => {
                write!(formatter, "base commit {oid} is not available locally")
            }
            Self::Git { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PullRequestFetchError {}

/// The ref [`fetch_pull_request`] writes the head of pull request `number` to.
pub fn pull_request_head_ref(number: u64) -> String {
    format!("refs/vigil/pr/{number}/head")
}

fn pull_request_base_ref(number: u64) -> String {
    format!("refs/vigil/pr/{number}/base")
}

/// Fetches the head of a pull request, and its base commit when missing, into
/// vigil-private refs. See the module docs for what it touches.
pub async fn fetch_pull_request(
    repo_root: &Path,
    request: &PullRequestFetch,
) -> Result<FetchedPullRequest, PullRequestFetchError> {
    let remote = select_remote(repo_root, &request.repository).await?;
    let head_ref = pull_request_head_ref(request.number);
    fetch_refspec(
        repo_root,
        &remote,
        &format!("+refs/pull/{}/head:{head_ref}", request.number),
    )
    .await?;
    let head_oid =
        resolve_commit(repo_root, &head_ref)
            .await
            .ok_or_else(|| PullRequestFetchError::Git {
                message: format!("{head_ref} is missing after fetch"),
            })?;

    let base_oid = request.base_oid.trim().to_string();
    if resolve_commit(repo_root, &base_oid).await.is_none() {
        let base_ref = pull_request_base_ref(request.number);
        let from_branch = format!("+refs/heads/{}:{base_ref}", request.base_ref_name.trim());
        // The base branch usually still contains the commit; if it was
        // force-pushed away, GitHub also serves reachable commits by id.
        if fetch_refspec(repo_root, &remote, &from_branch)
            .await
            .is_err()
            || resolve_commit(repo_root, &base_oid).await.is_none()
        {
            let by_id = format!("+{base_oid}:{base_ref}");
            let _ = fetch_refspec(repo_root, &remote, &by_id).await;
        }
        if resolve_commit(repo_root, &base_oid).await.is_none() {
            return Err(PullRequestFetchError::MissingBase { oid: base_oid });
        }
    }

    Ok(FetchedPullRequest {
        remote,
        head_ref,
        head_oid,
        base_oid,
    })
}

/// The pull request as [`fetch_pull_request`] would leave it, built from
/// commits already in the object store. Runs a few local git commands and
/// never touches the network.
///
/// Succeeds when `request.head_oid` and `request.base_oid` both resolve to
/// local commits and a remote can be picked the way a fetch picks one. If
/// [`pull_request_head_ref`] does not already point at the head, it is
/// written there, so the commits stay reachable exactly as after a fetch.
/// Returns `None`, touching nothing, when either commit is missing (or
/// another command fails); the caller should fetch instead.
///
/// The result is only as fresh as `request.head_oid`: commits pushed since
/// GitHub reported it are not picked up.
pub async fn resolve_local_pull_request(
    repo_root: &Path,
    request: &PullRequestFetch,
) -> Option<FetchedPullRequest> {
    let head_ref = pull_request_head_ref(request.number);
    let head_oid = request.head_oid.trim();
    let base_oid = request.base_oid.trim();
    if head_oid.is_empty() || base_oid.is_empty() {
        return None;
    }
    let (remote, ref_target, head, base) = tokio::join!(
        select_remote(repo_root, &request.repository),
        resolve_commit(repo_root, &head_ref),
        resolve_commit(repo_root, head_oid),
        resolve_commit(repo_root, base_oid),
    );
    let remote = remote.ok()?;
    // Full ids only: an abbreviated id could resolve to a different commit.
    let head = head.filter(|head| head.eq_ignore_ascii_case(head_oid))?;
    let base = base.filter(|base| base.eq_ignore_ascii_case(base_oid))?;
    if ref_target.as_deref() != Some(head.as_str()) {
        git_output(repo_root, &["update-ref", &head_ref, &head])
            .await
            .ok()?;
    }
    Some(FetchedPullRequest {
        remote,
        head_ref,
        head_oid: head,
        base_oid: base,
    })
}

/// Every vigil-private ref lives under this prefix; cleanup never deletes a
/// ref outside it.
const PULL_REQUEST_REFS: &str = "refs/vigil/pr/";

/// Deletes the refs [`fetch_pull_request`] wrote for pull request `number`
/// (`refs/vigil/pr/<number>/*`), once its review ends. Returns how many were
/// deleted. The commits stay in the object store until `git gc` finds them
/// unreachable.
pub async fn delete_pull_request_refs(repo_root: &Path, number: u64) -> color_eyre::Result<usize> {
    delete_refs_under(repo_root, &format!("{PULL_REQUEST_REFS}{number}/")).await
}

/// Deletes every `refs/vigil/pr/*` ref. Meant for startup, before any review
/// opens, to clear refs a previous session left behind (it quit mid-review
/// or crashed). Another vigil reviewing a pull request in the same
/// repository loses its ref too, which only matters if `git gc` runs before
/// that review ends; its diff is pinned to commit ids, not the ref.
pub async fn prune_pull_request_refs(repo_root: &Path) -> color_eyre::Result<usize> {
    delete_refs_under(repo_root, PULL_REQUEST_REFS).await
}

async fn delete_refs_under(repo_root: &Path, prefix: &str) -> color_eyre::Result<usize> {
    debug_assert!(prefix.starts_with(PULL_REQUEST_REFS));
    let listed = git_output(repo_root, &["for-each-ref", "--format=%(refname)", prefix]).await?;
    let refs = listed
        .lines()
        .map(str::trim)
        .filter(|name| name.starts_with(prefix) && name.starts_with(PULL_REQUEST_REFS))
        .collect::<Vec<_>>();
    if refs.is_empty() {
        return Ok(0);
    }
    let commands = refs
        .iter()
        .map(|name| format!("delete {name}\n"))
        .collect::<String>();
    git_output_with_stdin(
        repo_root,
        &["update-ref", "--stdin"],
        commands.as_bytes(),
        &[0],
    )
    .await?;
    Ok(refs.len())
}

async fn fetch_refspec(
    repo_root: &Path,
    remote: &str,
    refspec: &str,
) -> Result<(), PullRequestFetchError> {
    let output = git_output_raw(
        repo_root,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "--no-write-fetch-head",
            "--no-recurse-submodules",
            remote,
            refspec,
        ],
    )
    .await
    .map_err(|error| PullRequestFetchError::Git {
        message: error.to_string(),
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(PullRequestFetchError::Fetch {
            remote: remote.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

async fn resolve_commit(repo_root: &Path, revision: &str) -> Option<String> {
    let object = format!("{revision}^{{commit}}");
    let output = git_output_raw(repo_root, &["rev-parse", "--verify", "--quiet", &object])
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|oid| !oid.is_empty())
}

async fn select_remote(
    repo_root: &Path,
    repository: &RepositoryRef,
) -> Result<String, PullRequestFetchError> {
    // Unreadable remotes are treated as none, so the error names the
    // repository rather than the git failure.
    let remotes = list_remotes(repo_root).await.unwrap_or_default();
    choose_remote(&remotes, repository).ok_or_else(|| PullRequestFetchError::NoRemote {
        repository: repository.name_with_owner(),
    })
}

/// The remote pointing at `repository`, preferring `origin` then `upstream`
/// when several do; otherwise `origin` if it exists. Matches URLs as
/// configured, before `insteadOf` rewrites.
fn choose_remote(remotes: &[Remote], repository: &RepositoryRef) -> Option<String> {
    let matching = remotes
        .iter()
        .filter(|remote| url_points_at(&remote.url, repository))
        .map(|remote| remote.name.as_str())
        .collect::<Vec<_>>();
    let preferred = ["origin", "upstream"]
        .into_iter()
        .find(|name| matching.contains(name))
        .or_else(|| matching.first().copied());
    preferred
        .or_else(|| {
            remotes
                .iter()
                .any(|remote| remote.name == "origin")
                .then_some("origin")
        })
        .map(str::to_string)
}

/// Whether a remote URL names `repository`, in any of git's URL forms and
/// ignoring case.
fn url_points_at(url: &str, repository: &RepositoryRef) -> bool {
    parse_remote_url(url).is_some_and(|named| {
        named.host.eq_ignore_ascii_case(&repository.host)
            && named.owner.eq_ignore_ascii_case(&repository.owner)
            && named.name.eq_ignore_ascii_case(&repository.name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vigil() -> RepositoryRef {
        RepositoryRef {
            host: "github.com".to_string(),
            owner: "Scott-fo".to_string(),
            name: "vigil".to_string(),
        }
    }

    #[test]
    fn remote_urls_match_in_ssh_and_https_forms() {
        for url in [
            "git@github.com:Scott-fo/vigil.git",
            "git@github.com:scott-fo/vigil",
            "ssh://git@github.com/Scott-fo/vigil.git",
            "ssh://git@github.com:22/Scott-fo/vigil",
            "https://github.com/Scott-fo/vigil",
            "https://github.com/Scott-fo/vigil.git",
            "https://token@github.com/Scott-fo/vigil/",
            "git://github.com/Scott-fo/vigil.git",
        ] {
            assert!(url_points_at(url, &vigil()), "{url} should match");
        }
    }

    #[test]
    fn remote_urls_reject_other_repositories_and_local_paths() {
        for url in [
            "git@github.com:Scott-fo/vigil-fork.git",
            "git@github.com:someone/vigil.git",
            "https://gitlab.com/Scott-fo/vigil.git",
            "/tmp/remotes/vigil.git",
            "../vigil.git",
        ] {
            assert!(!url_points_at(url, &vigil()), "{url} should not match");
        }
    }

    fn remotes(pairs: &[(&str, &str)]) -> Vec<Remote> {
        pairs
            .iter()
            .map(|(name, url)| Remote {
                name: name.to_string(),
                url: url.to_string(),
                fetch_url: url.to_string(),
                gh_resolved: None,
            })
            .collect()
    }

    #[test]
    fn chooses_matching_remote_and_falls_back_to_origin() {
        let matching = remotes(&[
            ("origin", "git@github.com:me/vigil.git"),
            ("upstream", "https://github.com/Scott-fo/vigil.git"),
        ]);
        assert_eq!(
            choose_remote(&matching, &vigil()).as_deref(),
            Some("upstream")
        );

        let unrelated = remotes(&[("origin", "/tmp/remote.git")]);
        assert_eq!(
            choose_remote(&unrelated, &vigil()).as_deref(),
            Some("origin")
        );

        let no_origin = remotes(&[("mirror", "/tmp/remote.git")]);
        assert_eq!(choose_remote(&no_origin, &vigil()), None);
    }

    #[test]
    fn prefers_origin_when_several_remotes_match() {
        let matching = remotes(&[
            ("fork", "git@github.com:Scott-fo/vigil.git"),
            ("origin", "https://github.com/Scott-fo/vigil"),
        ]);
        assert_eq!(
            choose_remote(&matching, &vigil()).as_deref(),
            Some("origin")
        );
    }

    #[test]
    fn remotes_match_by_configured_url_not_the_rewritten_one() {
        let mut rewritten = remotes(&[("upstream", "git@github.com:Scott-fo/vigil.git")]);
        rewritten[0].fetch_url = "/srv/mirror/vigil.git".to_string();
        assert_eq!(
            choose_remote(&rewritten, &vigil()).as_deref(),
            Some("upstream")
        );
    }
}
