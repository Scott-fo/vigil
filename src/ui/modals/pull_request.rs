//! Review-action modals: the comment composer, submit review, and the
//! drafts list. Each draws a prepared [`PullRequestModalView`]; nothing here
//! decides what is allowed.

mod composer;
mod drafts;
mod submit;
mod text;

use ratatui::Frame;

use crate::app::PullRequestModalView;

pub(super) fn render_pull_request_modal(frame: &mut Frame, view: PullRequestModalView<'_>) {
    match view {
        PullRequestModalView::Composer { composer, posting } => {
            composer::render_composer(frame, composer, posting)
        }
        PullRequestModalView::Submit {
            form,
            number,
            draft_count,
            is_author,
            warnings,
            submitting,
        } => submit::render_submit(
            frame,
            submit::SubmitModal {
                form,
                number,
                draft_count,
                is_author,
                warnings: &warnings,
                submitting,
            },
        ),
        PullRequestModalView::Drafts { list, drafts } => {
            drafts::render_drafts(frame, list, &drafts)
        }
    }
}

/// `abc1234` from a full commit id.
fn short_oid(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}
