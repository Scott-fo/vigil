pub(super) fn scroll_to_make_row_visible(
    current_scroll: usize,
    viewport_height: usize,
    selected_row: usize,
) -> usize {
    if viewport_height == 0 {
        return current_scroll;
    }

    if selected_row < current_scroll {
        return selected_row;
    }

    let visible_end = current_scroll.saturating_add(viewport_height);
    if selected_row >= visible_end {
        return selected_row
            .saturating_add(1)
            .saturating_sub(viewport_height);
    }

    current_scroll
}

#[cfg(test)]
mod tests {
    use super::scroll_to_make_row_visible;

    #[test]
    fn keeps_visible_row_without_scrolling() {
        assert_eq!(scroll_to_make_row_visible(10, 5, 12), 10);
    }

    #[test]
    fn scrolls_up_to_selected_row() {
        assert_eq!(scroll_to_make_row_visible(10, 5, 8), 8);
    }

    #[test]
    fn scrolls_down_until_selected_row_is_last_visible_row() {
        assert_eq!(scroll_to_make_row_visible(10, 5, 16), 12);
    }

    #[test]
    fn zero_height_keeps_current_scroll() {
        assert_eq!(scroll_to_make_row_visible(10, 0, 16), 10);
    }
}
