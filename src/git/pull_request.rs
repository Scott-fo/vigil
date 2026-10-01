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
//! command here it runs with `GIT_TERMINAL_PROMPT=0`, so git's own credential
//! prompts fail instead of asking. ssh opens the terminal itself and may
//! still ask, as for the user's own `git fetch`; prefetches, which nobody
//! asked for, cannot (see below).
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
//!
//! # Prefetching
//!
//! [`prefetch_pull_requests`] makes [`resolve_local_pull_request`] hit before
//! anyone asks, for a batch of pull requests at once. It checks every head
//! and base with one `git cat-file --batch-check`, and only when something
//! is missing runs one `git fetch` per remote (a list's rows share one),
//! with a refspec per missing head and one per base branch a missing base
//! needs. A fetch is mostly handshake, so a batch costs about what one pull
//! request does, and a batch whose commits are all local costs one local
//! process and writes nothing. It writes the same head refs as
//! [`fetch_pull_request`]; base branches go to [`shared_base_ref`], one
//! per branch rather than per pull request.
//!
//! A prefetch is background work nobody asked for, so its fetch runs
//! unlike [`fetch_pull_request`]'s:
//!
//! - **Detached from the terminal.** It runs in a session of its own, so
//!   ssh cannot open `/dev/tty` to ask for a passphrase or to confirm a
//!   host key over vigil's screen. Unless the user configured their own
//!   ssh command (`GIT_SSH_COMMAND`, `GIT_SSH`, or `core.sshCommand`), ssh
//!   also runs with `BatchMode=yes`, so it fails instead of asking at all.
//!   An ssh agent that confirms each use (`ssh-add -c`, 1Password, a
//!   security key needing a touch) still asks, since that happens in the
//!   agent; the caller should stop after
//!   [`PullRequestFetchError::Access`] so it asks at most once.
//! - **Bounded.** It is terminated after [`PREFETCH_FETCH_TIMEOUT`], as
//!   when a laptop slept through it and left a dead connection, and fails
//!   with [`PullRequestFetchError::TimedOut`]. Dropping the future
//!   terminates it too.
//!
//! Terminating a fetch midway is safe: git writes the fetched pack under a
//! temporary name and moves refs only once it is complete, and it gets
//! `SIGTERM` first, which lets it remove its ref locks. An interrupted
//! fetch leaves at most an unused temporary file for `git gc`, never a
//! ref to missing objects. [`fetch_pull_request`], which the user asked
//! for and watches, is never terminated.
//!
//! # Ref lifecycle
//!
//! The refs under `refs/vigil/` only keep commits safe from `git gc`;
//! nothing reads them to find commits. [`prune_pull_request_refs`] deletes
//! them all at startup, before any list loads, so refs a prefetch writes
//! later last the session. [`delete_pull_request_refs`] deletes one pull
//! request's refs (`refs/vigil/pr/<n>/`) when its review ends; shared base
//! refs stay, since other pull requests may need them. Its commits stay in
//! the object store, so reopening it still resolves locally and writes the
//! head ref again. Once no ref holds them, `git gc` may prune the commits
//! after its grace period (two weeks by default); opening then fetches
//! again.

use std::{collections::HashSet, fmt, path::Path, time::Duration};

use super::{
    command::{
        DetachedOutput, git_output, git_output_detached, git_output_raw, git_output_with_stdin,
    },
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
    /// `git fetch` failed: offline, or the pull request ref does not exist
    /// on the remote.
    Fetch { remote: String, stderr: String },
    /// The remote refused the credentials, or its host key could not be
    /// verified. Retrying will not help until the user fixes their ssh or
    /// credential setup.
    Access { remote: String, stderr: String },
    /// A background fetch ran past its timeout and was terminated.
    TimedOut { remote: String },
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
            Self::Access { remote, stderr } => {
                write!(formatter, "{remote} refused access: {stderr}")
            }
            Self::TimedOut { remote } => {
                write!(formatter, "git fetch from {remote} timed out")
            }
            Self::MissingBase { oid } => {
                write!(formatter, "base commit {oid} is not available locally")
            }
            Self::Git { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PullRequestFetchError {}

/// What [`prefetch_pull_requests`] did for one pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchOutcome {
    /// The head and base were already local; nothing was fetched for it.
    AlreadyLocal,
    /// Fetched: [`resolve_local_pull_request`] now finds both commits.
    Fetched,
    /// Fetched, but the reported head or base is still missing: the head
    /// moved since GitHub reported it, or the base branch no longer holds
    /// the base commit. Opening it fetches as usual.
    Incomplete,
}

/// What a [`prefetch_pull_requests`] batch did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrefetchReport {
    /// One entry per distinct pull request asked for, in request order.
    pub outcomes: Vec<(u64, PrefetchOutcome)>,
    /// How many `git fetch` processes ran: none when everything was local,
    /// otherwise one per remote.
    pub fetches: usize,
}

impl PrefetchReport {
    /// The outcome for pull request `number`, if it was asked for.
    pub fn outcome(&self, number: u64) -> Option<PrefetchOutcome> {
        self.outcomes
            .iter()
            .find(|(candidate, _)| *candidate == number)
            .map(|(_, outcome)| *outcome)
    }
}

/// The ref [`fetch_pull_request`] writes the head of pull request `number` to.
pub fn pull_request_head_ref(number: u64) -> String {
    format!("refs/vigil/pr/{number}/head")
}

fn pull_request_base_ref(number: u64) -> String {
    format!("{PULL_REQUEST_REFS}{number}/base")
}

/// The ref [`prefetch_pull_requests`] writes base branch `branch` to. It
/// belongs to no one pull request, so ending a review leaves it; only the
/// startup prune deletes it.
pub fn shared_base_ref(branch: &str) -> String {
    format!("{SHARED_BASE_REFS}{branch}")
}

/// How long a background prefetch's `git fetch` may run before it is
/// terminated. A healthy fetch of ten pull requests takes seconds.
pub const PREFETCH_FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// Fetches the head of a pull request, and its base commit when missing, into
/// vigil-private refs. See the module docs for what it touches.
pub async fn fetch_pull_request(
    repo_root: &Path,
    request: &PullRequestFetch,
) -> Result<FetchedPullRequest, PullRequestFetchError> {
    let remote = select_remote(repo_root, &request.repository).await?;
    let head_ref = pull_request_head_ref(request.number);
    fetch_refspecs(
        repo_root,
        &remote,
        &[format!("+refs/pull/{}/head:{head_ref}", request.number)],
        FetchMode::Foreground,
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
        if fetch_refspecs(
            repo_root,
            &remote,
            std::slice::from_ref(&from_branch),
            FetchMode::Foreground,
        )
        .await
        .is_err()
            || resolve_commit(repo_root, &base_oid).await.is_none()
        {
            let by_id = format!("+{base_oid}:{base_ref}");
            let _ = fetch_refspecs(repo_root, &remote, &[by_id], FetchMode::Foreground).await;
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

/// Brings the commits of several pull requests into the object store
/// ahead of time, so opening them later resolves locally. See the module
/// docs for what it runs and writes.
///
/// Pull requests whose `head_oid` and `base_oid` are both local are left
/// alone. For the rest, one fetch per remote (picked as for
/// [`fetch_pull_request`]) brings in `refs/pull/<n>/head` where the head is
/// missing and the base branch where the base is, each base branch once
/// however many pull requests share it. Unlike [`fetch_pull_request`] it
/// never fetches a base commit by id: a pull request left
/// [`PrefetchOutcome::Incomplete`] is fetched in full when opened.
///
/// Fails when a remote cannot be picked or a fetch fails, after any earlier
/// remote's fetch has landed. A fetch fails as a whole when one of its refs
/// is missing on the remote.
pub async fn prefetch_pull_requests(
    repo_root: &Path,
    requests: &[PullRequestFetch],
) -> Result<PrefetchReport, PullRequestFetchError> {
    let mut seen = HashSet::new();
    let requests = requests
        .iter()
        .filter(|request| seen.insert(request.number))
        .collect::<Vec<_>>();

    let before = commits_present(repo_root, &requests).await?;
    let missing = requests
        .iter()
        .zip(&before)
        .filter(|(_, present)| !present.both())
        .map(|(request, present)| (*request, *present))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(PrefetchReport {
            outcomes: requests
                .iter()
                .map(|request| (request.number, PrefetchOutcome::AlreadyLocal))
                .collect(),
            fetches: 0,
        });
    }

    let mut fetches = 0;
    for (remote, group) in group_by_remote(repo_root, &missing).await? {
        let refspecs = prefetch_refspecs(&group);
        // Without refspecs `git fetch` would use the remote's configured
        // ones and move remote-tracking branches.
        if !refspecs.is_empty() {
            fetch_refspecs(repo_root, &remote, &refspecs, FetchMode::Background).await?;
            fetches += 1;
        }
    }

    let fetched = missing
        .iter()
        .map(|(request, _)| *request)
        .collect::<Vec<_>>();
    let mut after = commits_present(repo_root, &fetched).await?.into_iter();
    let outcomes = requests
        .iter()
        .zip(before)
        .map(|(request, present)| {
            let outcome = if present.both() {
                PrefetchOutcome::AlreadyLocal
            } else if after.next().is_some_and(CommitPresence::both) {
                PrefetchOutcome::Fetched
            } else {
                PrefetchOutcome::Incomplete
            };
            (request.number, outcome)
        })
        .collect();
    Ok(PrefetchReport { outcomes, fetches })
}

/// Which of a pull request's reported commits are in the object store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommitPresence {
    head: bool,
    base: bool,
}

impl CommitPresence {
    fn both(self) -> bool {
        self.head && self.base
    }
}

/// Checks every request's head and base with one `git cat-file`. Ids that
/// are not full object ids count as missing, as they do for
/// [`resolve_local_pull_request`].
async fn commits_present(
    repo_root: &Path,
    requests: &[&PullRequestFetch],
) -> Result<Vec<CommitPresence>, PullRequestFetchError> {
    let oids = requests
        .iter()
        .flat_map(|request| [request.head_oid.trim(), request.base_oid.trim()])
        .filter(|oid| is_full_oid(oid))
        .collect::<Vec<_>>();
    let mut commits = HashSet::new();
    if !oids.is_empty() {
        let stdin = oids
            .iter()
            .map(|oid| format!("{oid}\n"))
            .collect::<String>();
        let output = git_output_with_stdin(
            repo_root,
            &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
            stdin.as_bytes(),
            &[0],
        )
        .await
        .map_err(|error| PullRequestFetchError::Git {
            message: error.to_string(),
        })?;
        commits.extend(output.lines().filter_map(|line| {
            let (oid, kind) = line.split_once(' ')?;
            (kind == "commit").then(|| oid.to_ascii_lowercase())
        }));
    }
    let present = |oid: &str| is_full_oid(oid) && commits.contains(&oid.to_ascii_lowercase());
    Ok(requests
        .iter()
        .map(|request| CommitPresence {
            head: present(request.head_oid.trim()),
            base: present(request.base_oid.trim()),
        })
        .collect())
}

fn is_full_oid(oid: &str) -> bool {
    matches!(oid.len(), 40 | 64) && oid.chars().all(|c| c.is_ascii_hexdigit())
}

/// Missing pull requests of one remote, in request order.
type RemoteGroup<'a> = (String, Vec<(&'a PullRequestFetch, CommitPresence)>);

/// Groups missing pull requests by the remote their repository resolves
/// to. A list's rows share one repository, so this is almost always one
/// group.
async fn group_by_remote<'a>(
    repo_root: &Path,
    missing: &[(&'a PullRequestFetch, CommitPresence)],
) -> Result<Vec<RemoteGroup<'a>>, PullRequestFetchError> {
    let mut remotes: Vec<(&RepositoryRef, String)> = Vec::new();
    let mut groups: Vec<RemoteGroup<'a>> = Vec::new();
    for (request, present) in missing {
        let known = remotes
            .iter()
            .find(|(repository, _)| **repository == request.repository)
            .map(|(_, remote)| remote.clone());
        let remote = match known {
            Some(remote) => remote,
            None => {
                let remote = select_remote(repo_root, &request.repository).await?;
                remotes.push((&request.repository, remote.clone()));
                remote
            }
        };
        match groups.iter_mut().find(|(name, _)| *name == remote) {
            Some((_, group)) => group.push((request, *present)),
            None => groups.push((remote, vec![(request, *present)])),
        }
    }
    Ok(groups)
}

/// Refspecs fetching each missing head, and once each base branch a
/// missing base needs, into vigil-private refs. A base branch lands in
/// its [`shared_base_ref`], however many pull requests share it.
fn prefetch_refspecs(group: &[(&PullRequestFetch, CommitPresence)]) -> Vec<String> {
    let mut refspecs = Vec::new();
    let mut base_branches: Vec<&str> = Vec::new();
    for (request, present) in group {
        if !present.head {
            refspecs.push(format!(
                "+refs/pull/{}/head:{}",
                request.number,
                pull_request_head_ref(request.number)
            ));
        }
        let branch = request.base_ref_name.trim();
        if !present.base && is_plain_branch_name(branch) && !base_branches.contains(&branch) {
            base_branches.push(branch);
            refspecs.push(format!("+refs/heads/{branch}:{}", shared_base_ref(branch)));
        }
    }
    refspecs
}

/// Whether `name` can go into a refspec as is. GitHub only reports valid
/// branch names; this keeps a malformed one from changing the refspec.
fn is_plain_branch_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '+', '/'])
        && !name.contains("..")
        && !name.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, ':' | '^' | '~' | '?' | '*' | '[' | '\\')
        })
}

/// Refs of one pull request: `<prefix><number>/head` and `/base`.
const PULL_REQUEST_REFS: &str = "refs/vigil/pr/";

/// Base branches a prefetch brought in: `<prefix><branch>`.
const SHARED_BASE_REFS: &str = "refs/vigil/base/";

/// Every vigil-private ref lives under one of these prefixes; cleanup never
/// deletes a ref outside them.
const VIGIL_REFS: [&str; 2] = [PULL_REQUEST_REFS, SHARED_BASE_REFS];

/// Deletes the refs [`fetch_pull_request`] wrote for pull request `number`
/// (`refs/vigil/pr/<number>/*`), once its review ends. Returns how many were
/// deleted. Shared base refs stay. The commits stay in the object store
/// until `git gc` finds them unreachable.
pub async fn delete_pull_request_refs(repo_root: &Path, number: u64) -> color_eyre::Result<usize> {
    delete_refs_under(repo_root, &[&format!("{PULL_REQUEST_REFS}{number}/")]).await
}

/// Deletes every vigil-private ref: each pull request's
/// (`refs/vigil/pr/*`) and the shared base branches (`refs/vigil/base/*`).
/// Meant for startup, before any review opens or list loads, to clear refs
/// a previous session left behind (it quit mid-review or crashed). Another
/// vigil reviewing a pull request in the same repository loses its ref too,
/// which only matters if `git gc` runs before that review ends; its diff is
/// pinned to commit ids, not the ref.
pub async fn prune_pull_request_refs(repo_root: &Path) -> color_eyre::Result<usize> {
    delete_refs_under(repo_root, &VIGIL_REFS).await
}

async fn delete_refs_under(repo_root: &Path, prefixes: &[&str]) -> color_eyre::Result<usize> {
    let owned = |name: &str| VIGIL_REFS.iter().any(|vigil| name.starts_with(vigil));
    debug_assert!(prefixes.iter().all(|prefix| owned(prefix)));
    let mut args = vec!["for-each-ref", "--format=%(refname)"];
    args.extend(prefixes);
    let listed = git_output(repo_root, &args).await?;
    let refs = listed
        .lines()
        .map(str::trim)
        .filter(|name| owned(name) && prefixes.iter().any(|prefix| name.starts_with(prefix)))
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

/// Who a fetch is for, which decides how it may behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchMode {
    /// The user asked and is watching: it runs as long as it takes, and
    /// ssh may prompt as it would for their own `git fetch`.
    Foreground,
    /// Nobody asked: detached from the terminal, batch-mode ssh unless the
    /// user configured their own, and terminated after
    /// [`PREFETCH_FETCH_TIMEOUT`]. See the module docs.
    Background,
}

/// One `git fetch` of every refspec from `remote`: one handshake however
/// many refs it brings in. Every fetch here goes through it.
async fn fetch_refspecs(
    repo_root: &Path,
    remote: &str,
    refspecs: &[String],
    mode: FetchMode,
) -> Result<(), PullRequestFetchError> {
    let mut args = vec![
        "fetch",
        "--quiet",
        "--no-tags",
        "--no-write-fetch-head",
        "--no-recurse-submodules",
        // Without this, fetching `refs/heads/<base>` also moves the
        // remote-tracking branch the remote's configured refspec maps it
        // to; only the explicit refspecs may be written.
        "--refmap=",
        remote,
    ];
    args.extend(refspecs.iter().map(String::as_str));
    let git_error = |error: color_eyre::Report| PullRequestFetchError::Git {
        message: error.to_string(),
    };
    let output = match mode {
        FetchMode::Foreground => git_output_raw(repo_root, &args).await.map_err(git_error)?,
        FetchMode::Background => {
            let ssh = SshConfig::read(repo_root).await;
            let envs = ssh.batch_mode_env().into_iter().collect::<Vec<_>>();
            match git_output_detached(repo_root, &args, &envs, PREFETCH_FETCH_TIMEOUT)
                .await
                .map_err(git_error)?
            {
                DetachedOutput::Finished(output) => output,
                DetachedOutput::TimedOut => {
                    return Err(PullRequestFetchError::TimedOut {
                        remote: remote.to_string(),
                    });
                }
            }
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let remote = remote.to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if refuses_access(&stderr) {
        Err(PullRequestFetchError::Access { remote, stderr })
    } else {
        Err(PullRequestFetchError::Fetch { remote, stderr })
    }
}

/// Whether a failed fetch's stderr says the remote refused credentials or
/// its host could not be verified, which retrying cannot fix.
fn refuses_access(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    [
        "permission denied",
        "host key verification failed",
        "authentication failed",
        "no more authentication methods",
        "too many authentication failures",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "agent refused operation",
        "sign_and_send_pubkey",
    ]
    .iter()
    .any(|marker| stderr.contains(marker))
}

/// The user's own ssh setup, which a background fetch must not override.
#[derive(Debug, Default)]
struct SshConfig {
    /// `GIT_SSH_COMMAND` is set.
    env_command: bool,
    /// `GIT_SSH` is set.
    env_program: bool,
    /// `core.sshCommand` is configured for the repository.
    config_command: bool,
}

impl SshConfig {
    async fn read(repo_root: &Path) -> Self {
        let config_command = git_output_raw(repo_root, &["config", "--get", "core.sshCommand"])
            .await
            .is_ok_and(|output| output.status.success());
        Self {
            env_command: std::env::var_os("GIT_SSH_COMMAND").is_some(),
            env_program: std::env::var_os("GIT_SSH").is_some(),
            config_command,
        }
    }

    /// `BatchMode=yes` for ssh, so a background fetch fails instead of
    /// prompting, unless the user chose their own ssh command.
    fn batch_mode_env(&self) -> Option<(&'static str, &'static str)> {
        (!self.env_command && !self.env_program && !self.config_command)
            .then_some(("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"))
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

    fn fetch(number: u64, base_ref_name: &str) -> PullRequestFetch {
        PullRequestFetch {
            repository: vigil(),
            number,
            head_oid: "a".repeat(40),
            base_oid: "b".repeat(40),
            base_ref_name: base_ref_name.to_string(),
        }
    }

    #[test]
    fn prefetch_asks_for_missing_heads_and_each_needed_base_branch_once() {
        let (one, two, three, four) = (
            fetch(1, "main"),
            fetch(2, "main"),
            fetch(3, "release/1.0"),
            fetch(4, "bad:ref"),
        );
        let missing = |head, base| CommitPresence { head, base };
        let group = [
            (&one, missing(false, false)),
            (&two, missing(false, false)),
            (&three, missing(true, false)),
            (&four, missing(false, false)),
        ];
        assert_eq!(
            prefetch_refspecs(&group),
            vec![
                "+refs/pull/1/head:refs/vigil/pr/1/head",
                "+refs/heads/main:refs/vigil/base/main",
                "+refs/pull/2/head:refs/vigil/pr/2/head",
                "+refs/heads/release/1.0:refs/vigil/base/release/1.0",
                "+refs/pull/4/head:refs/vigil/pr/4/head",
            ]
        );
    }

    #[test]
    fn background_fetches_use_batch_mode_ssh_only_without_the_users_own() {
        let none = SshConfig::default();
        assert_eq!(
            none.batch_mode_env(),
            Some(("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"))
        );
        for own in [
            SshConfig {
                env_command: true,
                ..SshConfig::default()
            },
            SshConfig {
                env_program: true,
                ..SshConfig::default()
            },
            SshConfig {
                config_command: true,
                ..SshConfig::default()
            },
        ] {
            assert_eq!(own.batch_mode_env(), None, "{own:?}");
        }
    }

    #[test]
    fn refused_credentials_and_unknown_hosts_are_access_failures() {
        for stderr in [
            "git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository.",
            "Host key verification failed.\nfatal: Could not read from remote repository.",
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            "remote: Invalid username or token.\nfatal: Authentication failed for 'https://github.com/acme/widgets.git/'",
            "sign_and_send_pubkey: signing failed for ED25519 \"key\" from agent: agent refused operation",
        ] {
            assert!(refuses_access(stderr), "{stderr}");
        }
        for stderr in [
            "ssh: connect to host github.com port 22: Operation timed out\nfatal: Could not read from remote repository.",
            "fatal: couldn't find remote ref refs/pull/9/head",
            "fatal: unable to access 'https://github.com/acme/widgets.git/': Could not resolve host: github.com",
        ] {
            assert!(!refuses_access(stderr), "{stderr}");
        }
    }

    #[test]
    fn only_plain_branch_names_reach_a_refspec() {
        for name in ["main", "release/1.0", "feature_x-2"] {
            assert!(is_plain_branch_name(name), "{name}");
        }
        for name in ["", "a:b", "-x", "+x", "a b", "a..b", "a^", "*", "/abs"] {
            assert!(!is_plain_branch_name(name), "{name:?}");
        }
        assert!(is_full_oid(&"A1".repeat(20)));
        assert!(!is_full_oid("a1b2c3"));
        assert!(!is_full_oid(&"g".repeat(40)));
    }
}
