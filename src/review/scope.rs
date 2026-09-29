/// What is being reviewed within one repository.
///
/// Scopes carry only the identity of a review target, not a resolved snapshot:
/// the same scope keeps naming the same target while the refs it mentions move.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ReviewScope {
    /// Uncommitted changes in the working tree.
    WorkingTree,
    /// A single commit, compared against its base.
    CommitCompare { commit_hash: String },
    /// The changes on `source_ref` relative to `destination_ref`.
    BranchCompare {
        source_ref: String,
        destination_ref: String,
    },
}
