use ratatui::layout::{Constraint, Direction, Layout, Rect};

const SIDEBAR_WIDTH: u16 = 32;

pub(super) fn main_layout(area: Rect, sidebar_hidden: bool) -> [Rect; 2] {
    if sidebar_hidden {
        return [Rect::new(area.x, area.y, 0, area.height), area];
    }

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(40)])
        .split(area);

    [layout[0], layout[1]]
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
    fn main_layout_gives_diff_full_width_when_sidebar_is_hidden() {
        let area = Rect::new(0, 0, 120, 40);
        let [sidebar_area, diff_area] = main_layout(area, true);

        assert_eq!(sidebar_area.width, 0);
        assert_eq!(diff_area, area);
    }

    #[test]
    fn main_layout_uses_compact_sidebar_width() {
        let area = Rect::new(0, 0, 120, 40);
        let [sidebar_area, diff_area] = main_layout(area, false);

        assert_eq!(sidebar_area.width, SIDEBAR_WIDTH);
        assert_eq!(diff_area.width, area.width - SIDEBAR_WIDTH);
    }
}
