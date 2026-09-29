//! Pull request rendering: the list screen, the overview page, review thread
//! boxes in the diff, and the compact chips the footer uses.
//!
//! Everything here draws prepared app state (`App::pull_request_overview`,
//! `App::review_thread_rows_at`, `forge` summaries); nothing loads or
//! decides. Relative times are computed against the wall clock at draw time.

mod labels;
mod list;
mod markdown;
mod overview;
mod thread;

pub(super) use self::labels::{now_unix_seconds, pull_request_chip_spans};
pub use self::list::PullRequestListTarget;
pub(super) use self::list::{list_target_at, render_pull_request_list};
pub(super) use self::overview::{render_overview_body, render_overview_header};
pub(super) use self::thread::thread_row_line;
