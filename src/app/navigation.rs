pub(super) fn clamp_index(index: usize, item_count: usize) -> usize {
    index.min(item_count.saturating_sub(1))
}

pub(super) fn move_index(index: usize, item_count: usize, delta: i32) -> usize {
    if item_count == 0 {
        return 0;
    }

    let current = clamp_index(index, item_count);
    offset_usize(current, delta).min(item_count - 1)
}

pub(super) fn scroll_u16(offset: u16, delta: i32) -> u16 {
    if delta.is_negative() {
        offset.saturating_sub(delta.unsigned_abs() as u16)
    } else {
        offset.saturating_add(delta as u16)
    }
}

pub(super) fn scroll_usize(offset: usize, delta: i32) -> usize {
    offset_usize(offset, delta)
}

fn offset_usize(value: usize, delta: i32) -> usize {
    if delta.is_negative() {
        value.saturating_sub(delta.unsigned_abs() as usize)
    } else {
        value.saturating_add(delta as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::{clamp_index, move_index, scroll_u16, scroll_usize};

    #[test]
    fn clamp_index_handles_empty_and_overlarge_indices() {
        assert_eq!(clamp_index(4, 0), 0);
        assert_eq!(clamp_index(4, 3), 2);
        assert_eq!(clamp_index(1, 3), 1);
    }

    #[test]
    fn move_index_saturates_at_list_edges() {
        assert_eq!(move_index(0, 0, 1), 0);
        assert_eq!(move_index(0, 4, -1), 0);
        assert_eq!(move_index(1, 4, 2), 3);
        assert_eq!(move_index(3, 4, 10), 3);
        assert_eq!(move_index(10, 4, -1), 2);
    }

    #[test]
    fn scroll_offsets_saturate_without_upper_bound() {
        assert_eq!(scroll_u16(2, -4), 0);
        assert_eq!(scroll_u16(u16::MAX - 1, 10), u16::MAX);
        assert_eq!(scroll_usize(2, -4), 0);
        assert_eq!(scroll_usize(usize::MAX - 1, 10), usize::MAX);
    }
}
