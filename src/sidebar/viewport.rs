#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeRange {
    pub start: usize,
    pub end: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeViewportMetrics {
    pub item_count: usize,
    pub item_height: usize,
    pub scroll_top: usize,
    pub viewport_height: usize,
    pub overscan: usize,
}

pub fn compute_window_range(
    metrics: FileTreeViewportMetrics,
    current_range: Option<FileTreeRange>,
) -> FileTreeRange {
    let visible_range = compute_visible_range(&metrics);
    let normalized_current = current_range
        .and_then(|range| normalize_range(range, metrics.item_count))
        .unwrap_or(FileTreeRange {
            start: 0,
            end: None,
        });

    if let (Some(visible_end), Some(current_end)) = (visible_range.end, normalized_current.end)
        && visible_range.start >= normalized_current.start
        && visible_end <= current_end
    {
        return normalized_current;
    }

    expand_range(visible_range, metrics.item_count, metrics.overscan)
}

fn compute_visible_range(metrics: &FileTreeViewportMetrics) -> FileTreeRange {
    if metrics.item_count == 0 || metrics.item_height == 0 {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    }

    let raw_start = metrics.scroll_top / metrics.item_height;
    let raw_end = metrics
        .scroll_top
        .saturating_add(metrics.viewport_height)
        .saturating_add(metrics.item_height.saturating_sub(1))
        / metrics.item_height;
    let raw_end = raw_end.saturating_sub(1);
    if raw_end < raw_start || raw_start >= metrics.item_count {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    }

    FileTreeRange {
        start: raw_start,
        end: Some(raw_end.min(metrics.item_count - 1)),
    }
}

fn normalize_range(range: FileTreeRange, item_count: usize) -> Option<FileTreeRange> {
    let end = range.end?;
    if item_count == 0 || end < range.start {
        return None;
    }
    let start = range.start.min(item_count - 1);
    Some(FileTreeRange {
        start,
        end: Some(end.max(start).min(item_count - 1)),
    })
}

fn expand_range(range: FileTreeRange, item_count: usize, overscan: usize) -> FileTreeRange {
    let Some(end) = range.end else {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    };
    if item_count == 0 || end < range.start {
        return FileTreeRange {
            start: 0,
            end: None,
        };
    }

    FileTreeRange {
        start: range.start.saturating_sub(overscan),
        end: Some(end.saturating_add(overscan).min(item_count - 1)),
    }
}
