//! Review-action modals: the comment composer and the drafts list. Each
//! draws a prepared [`PullRequestModalView`]; nothing here decides what is
//! allowed.

mod composer;
mod drafts;
mod text;

use ratatui::Frame;

use crate::app::PullRequestModalView;

pub(super) fn render_pull_request_modal(frame: &mut Frame, view: PullRequestModalView<'_>) {
    match view {
        PullRequestModalView::Composer { composer } => composer::render_composer(frame, composer),
        PullRequestModalView::Drafts { list, drafts } => {
            drafts::render_drafts(frame, list, &drafts)
        }
    }
}
