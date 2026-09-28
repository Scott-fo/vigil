//! Branch management and remote sync.
//!
//! This module owns the repository's branch state as the app sees it: the
//! [`git::BranchSnapshot`](crate::git::BranchSnapshot) behind the footer's
//! branch readout, the branch panel, and the single git branch operation that
//! may be running.
//!
//! - The snapshot reloads in the background whenever the review refreshes,
//!   when the panel opens, and after every operation. Stale loads are dropped
//!   by request id. Branch switches made outside vigil show up once the
//!   working tree changes.
//! - The panel is a small state machine ([`BranchPanelMode`]): browse the
//!   list, filter it, name a new or renamed branch, or confirm a delete. UI
//!   code renders a prepared [`BranchPanelView`] and never runs git.
//! - One operation runs at a time. `p`, `P`, and the panel share it; a second
//!   request while one runs is refused, not queued. Operations that change
//!   the checked-out files reload the review when they finish.

mod key;
mod operation;
mod panel;
mod status;

pub use self::panel::{
    BranchPanel, BranchPanelMode, BranchPanelRow, BranchPanelView, BranchSection,
};
pub(super) use self::status::BranchStatus;

#[cfg(test)]
mod tests;
