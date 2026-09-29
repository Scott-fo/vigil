//! Review-action modals: the comment composer, submit review, merge, the
//! actions menu, and the drafts list. Each draws a prepared
//! [`PullRequestModalView`]; nothing here decides what is allowed.

mod actions;
mod composer;
mod drafts;
mod merge;
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
        PullRequestModalView::Merge {
            form,
            detail,
            blockers,
            auto_merge,
            head_oid,
            has_new_commits,
            merging,
        } => merge::render_merge(
            frame,
            merge::MergeModal {
                form,
                detail,
                blockers: &blockers,
                auto_merge,
                head_oid,
                has_new_commits,
                merging,
            },
        ),
        PullRequestModalView::Actions {
            menu,
            number,
            updating,
        } => actions::render_actions(frame, menu, number, updating),
        PullRequestModalView::Drafts { list, drafts } => {
            drafts::render_drafts(frame, list, &drafts)
        }
    }
}

/// `abc1234` from a full commit id.
fn short_oid(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}
