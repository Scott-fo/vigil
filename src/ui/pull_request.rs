//! Pull request rendering: the overview page, review thread boxes in the
//! diff, and the compact chips the footer and lists use.
//!
//! Everything here draws prepared app state (`App::pull_request_overview`,
//! `App::review_thread_rows_at`, `forge` summaries); nothing loads or
//! decides. Relative times are computed against the wall clock at draw time.

mod labels;
mod markdown;
mod overview;
mod thread;

pub(super) use self::labels::{now_unix_seconds, pull_request_chip_spans};
pub(super) use self::overview::{render_overview_body, render_overview_header};
pub(super) use self::thread::thread_row_line;
