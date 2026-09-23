use ratatui::layout::{Constraint, Direction, Layout, Rect};

const SIDEBAR_WIDTH: u16 = 32;

/// Screen regions for the main review view.
///
/// Rendering and mouse hit testing both resolve regions through this type, so
/// they cannot disagree about where the sidebar list or diff body begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ScreenLayout {
    pub(super) header: Rect,
    pub(super) sidebar: Option<SidebarLayout>,
    pub(super) diff: DiffLayout,
    pub(super) footer: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SidebarLayout {
    /// The whole sidebar column, used for pane hover detection.
    pub(super) area: Rect,
    pub(super) title: Rect,
    pub(super) list: Rect,
    /// One-column rule between sidebar and diff; doubles as the sidebar
    /// scrollbar track. Belongs to neither pane for hit testing.
    pub(super) divider: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DiffLayout {
    /// The whole diff column, used for pane hover detection.
    pub(super) area: Rect,
    pub(super) header: Rect,
    pub(super) body: Rect,
}

impl ScreenLayout {
    pub(super) fn new(area: Rect, sidebar_hidden: bool) -> Self {
        let [header, main, footer] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .areas(area);

        let (sidebar, diff_area) = if sidebar_hidden {
            (None, main)
        } else {
            let [sidebar_area, divider, diff_area] = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(SIDEBAR_WIDTH),
                    Constraint::Length(1),
                    Constraint::Min(40),
                ])
                .areas(main);
            let [title, list] = split_title_row(sidebar_area);
            (
                Some(SidebarLayout {
                    area: sidebar_area,
                    title,
                    list,
                    divider,
                }),
                diff_area,
            )
        };

        let [diff_header, diff_body] = split_title_row(diff_area);
        Self {
            header,
            sidebar,
            diff: DiffLayout {
                area: diff_area,
                header: diff_header,
                body: diff_body,
            },
            footer,
        }
    }
}

fn split_title_row(area: Rect) -> [Rect; 2] {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .areas(area)
}

pub(super) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let popup_width = width.min(area.width.saturating_sub(2)).max(1);
    let popup_height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(popup_width) / 2,
        area.y + area.height.saturating_sub(popup_height) / 2,
        popup_width,
        popup_height,
    )
}

pub(super) fn top_right_rect(width: u16, height: u16, top: u16, area: Rect) -> Rect {
    let popup_width = width.min(area.width.saturating_sub(2)).max(1);
    let popup_height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(popup_width).saturating_sub(1),
        top.min(area.y + area.height.saturating_sub(popup_height)),
        popup_width,
        popup_height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_takes_full_width_when_sidebar_is_hidden() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = ScreenLayout::new(area, true);

        assert_eq!(layout.sidebar, None);
        assert_eq!(layout.diff.area.width, area.width);
    }

    #[test]
    fn sidebar_keeps_a_fixed_compact_width() {
        let layout = ScreenLayout::new(Rect::new(0, 0, 200, 40), false);
        let sidebar = layout.sidebar.expect("sidebar should be visible");

        assert_eq!(sidebar.area.width, SIDEBAR_WIDTH);
        assert_eq!(sidebar.divider.x, SIDEBAR_WIDTH);
        assert_eq!(layout.diff.area.x, SIDEBAR_WIDTH + 1);
        assert_eq!(layout.diff.area.width, 200 - SIDEBAR_WIDTH - 1);
    }

    #[test]
    fn header_and_footer_frame_the_panes() {
        let layout = ScreenLayout::new(Rect::new(0, 0, 120, 40), false);
        let sidebar = layout.sidebar.expect("sidebar should be visible");

        assert_eq!(layout.header, Rect::new(0, 0, 120, 1));
        assert_eq!(layout.footer, Rect::new(0, 39, 120, 1));
        assert_eq!(sidebar.list.y, 2);
        assert_eq!(layout.diff.body.y, 2);
        assert_eq!(sidebar.list.bottom(), layout.footer.y);
        assert_eq!(layout.diff.body.bottom(), layout.footer.y);
    }
}
