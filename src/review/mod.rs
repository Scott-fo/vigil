//! Code review sessions, reports, and persisted findings.
//!
//! This module owns Vigil's review contract with external agents. Callers build
//! a repository snapshot, send it to a provider such as the Codex app server,
//! then persist the structured summary and file comments returned by that
//! provider. Reviews are tied to a snapshot id so UI code can tell fresh
//! comments from stale comments after the working tree or compared refs move.

mod annotations;
mod codex;
mod report;
mod snapshot;
mod store;
mod target;

pub use self::annotations::{ReviewDisplayComment, comments_for_display_line};
pub use self::codex::{CodexAppReviewProvider, ProviderReview, ReviewProvider};
pub use self::report::{
    ReviewFinding, ReviewFindingState, ReviewReport, ReviewSeverity, ReviewSide, ReviewSummary,
    ReviewVerdict, parse_review_report, review_report_json_schema,
};
pub use self::snapshot::{BuildReviewSnapshotOptions, build_review_snapshot};
pub use self::store::{PersistedReview, ReviewStore, default_database_path};
pub use self::target::{ReviewScope, ReviewSnapshot, ReviewTarget};
