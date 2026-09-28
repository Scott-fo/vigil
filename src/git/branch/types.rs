use std::time::SystemTime;

/// Point-in-time view of the repository's branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSnapshot {
    pub head: HeadState,
    /// Local branches then remote branches, each most recently committed
    /// first, as `git for-each-ref` reports them.
    pub branches: Vec<BranchEntry>,
    /// The branch `git switch -` would return to, when it still exists.
    pub previous_branch: Option<String>,
    pub remotes: Vec<String>,
    /// When any remote was last fetched from, if ever.
    pub last_fetch: Option<SystemTime>,
    /// A multi-step git operation left in progress, such as a conflicted merge.
    pub operation: Option<RepoOperation>,
}

impl BranchSnapshot {
    /// The checked-out local branch, unless HEAD is detached or unborn.
    pub fn head_branch(&self) -> Option<&BranchEntry> {
        self.branches.iter().find(|branch| branch.is_head)
    }

    pub fn local_branch(&self, name: &str) -> Option<&BranchEntry> {
        self.branches
            .iter()
            .find(|branch| branch.location == BranchLocation::Local && branch.name == name)
    }
}

/// What HEAD points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    /// A branch with no commits yet, as in a freshly initialized repository.
    Unborn(String),
    Detached {
        short_hash: String,
    },
}

impl HeadState {
    /// The checked-out branch name, including an unborn one.
    pub fn branch_name(&self) -> Option<&str> {
        match self {
            Self::Branch(name) | Self::Unborn(name) => Some(name),
            Self::Detached { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchLocation {
    Local,
    Remote { remote: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEntry {
    /// Short name as git prints it: `main`, `origin/feature`.
    pub name: String,
    pub location: BranchLocation,
    pub is_head: bool,
    /// Configured upstream of a local branch. Always `None` for remote
    /// branches.
    pub upstream: Option<Upstream>,
    pub tip: BranchTip,
}

impl BranchEntry {
    pub fn is_remote(&self) -> bool {
        matches!(self.location, BranchLocation::Remote { .. })
    }

    /// Name the branch has, or would get, as a local branch:
    /// `origin/feature` checks out as `feature`.
    pub fn local_name(&self) -> &str {
        match &self.location {
            BranchLocation::Local => &self.name,
            BranchLocation::Remote { remote } => self
                .name
                .strip_prefix(remote.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or(&self.name),
        }
    }
}

/// The commit a branch points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTip {
    pub short_hash: String,
    pub subject: String,
    /// Committer date in seconds since the Unix epoch.
    pub committed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    /// Short upstream name, such as `origin/main`.
    pub name: String,
    pub divergence: Divergence,
}

/// How a local branch relates to its upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Divergence {
    /// Commits only on the local branch (`ahead`) and only on the upstream
    /// (`behind`). Both zero means in sync.
    Tracking { ahead: usize, behind: usize },
    /// The upstream ref no longer exists, usually because the remote branch
    /// was deleted and pruned.
    Gone,
}

/// A multi-step operation git is in the middle of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoOperation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}
