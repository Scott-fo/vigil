use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{DiffLineWrapMode, DiffViewMode},
    ui,
};

use super::{
    DiffDisplayLineAnchor, DiffHunkGap, DiffLineKind, DiffSelectionPane, DiffSelectionPoint,
    DiffView, GapExpandDirection, SyntaxToken,
    rendering::{
        normalize_selection_points, render_expand_gap_line, render_expanded_context_lines,
        render_split_hunk_rows, render_split_pair_lines, render_unified_code_lines,
        slice_string_by_width,
    },
};

impl DiffView {
    pub fn rendered_lines(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> &[Line<'static>] {
        self.ensure_display_cache(mode, width, line_wrap);
        &self.display_cache.entry(mode).lines
    }

    pub fn rendered_lines_window(
        &self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        start: usize,
        end: usize,
    ) -> Vec<Line<'static>> {
        if start >= end {
            return Vec::new();
        }

        if self.rows.is_empty() {
            return if start == 0 {
                vec![Line::from(Span::styled(
                    self.note
                        .clone()
                        .unwrap_or_else(|| "No textual diff available.".to_string()),
                    ui::diff_meta_style(),
                ))]
            } else {
                Vec::new()
            };
        }

        let mut cursor = 0usize;
        let mut lines = Vec::with_capacity(end.saturating_sub(start));

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            match mode {
                DiffViewMode::Unified => {
                    for row_index in hunk.row_start..hunk.row_end {
                        for rendered_line in
                            render_unified_code_lines(&self.rows[row_index], width, line_wrap)
                        {
                            if push_window_line(
                                &mut lines,
                                &mut cursor,
                                start,
                                end,
                                rendered_line.line,
                            ) {
                                return lines;
                            }
                        }
                    }
                }
                DiffViewMode::Split => {
                    self.push_split_window_lines(
                        hunk.row_start,
                        hunk.row_end,
                        width,
                        line_wrap,
                        &mut cursor,
                        start,
                        end,
                        &mut lines,
                    );
                    if cursor >= end {
                        return lines;
                    }
                }
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_window_lines(
                    gap,
                    width,
                    mode,
                    line_wrap,
                    &mut cursor,
                    start,
                    end,
                    &mut lines,
                );
                if cursor >= end {
                    return lines;
                }
            }
        }

        lines
    }

    pub fn estimated_display_line_count(
        &self,
        mode: DiffViewMode,
        line_wrap: DiffLineWrapMode,
    ) -> usize {
        if self.rows.is_empty() {
            return 1;
        }

        let row_count = match (mode, line_wrap) {
            (DiffViewMode::Split, DiffLineWrapMode::NoWrap) => {
                self.estimated_split_no_wrap_row_count()
            }
            _ => self.rows.len(),
        };

        row_count
            + self
                .gaps
                .iter()
                .map(|gap| {
                    let expansion = self
                        .gap_expansions
                        .get(&gap.gap_index)
                        .copied()
                        .unwrap_or_default();
                    let expanded = expansion
                        .from_previous
                        .saturating_add(expansion.from_next)
                        .min(gap.new_count);
                    let remaining = gap.new_count.saturating_sub(expanded);
                    expanded + if remaining > 0 { 2 } else { 0 }
                })
                .sum::<usize>()
    }

    fn estimated_split_no_wrap_row_count(&self) -> usize {
        let mut line_count = 0usize;

        for hunk in &self.hunks {
            let mut row_index = hunk.row_start;
            while row_index < hunk.row_end {
                match self.rows[row_index].kind {
                    DiffLineKind::Removed | DiffLineKind::Added => {
                        let mut removed_count = 0usize;
                        let mut added_count = 0usize;
                        while row_index < hunk.row_end {
                            match self.rows[row_index].kind {
                                DiffLineKind::Removed => removed_count += 1,
                                DiffLineKind::Added => added_count += 1,
                                _ => break,
                            }
                            row_index += 1;
                        }
                        line_count += removed_count.max(added_count);
                    }
                    DiffLineKind::Context
                    | DiffLineKind::ConflictAction
                    | DiffLineKind::ConflictMarker(_) => {
                        line_count += 1;
                        row_index += 1;
                    }
                }
            }
        }

        line_count
    }

    pub fn selection_point_at(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        display_index: usize,
        column: usize,
    ) -> Option<DiffSelectionPoint> {
        self.ensure_display_cache(mode, width, line_wrap);
        let selection_line = self
            .display_cache
            .entry(mode)
            .selection
            .get(display_index)?;
        let (pane, segment) = match mode {
            DiffViewMode::Unified => (
                DiffSelectionPane::Unified,
                selection_line
                    .unified
                    .as_ref()
                    .filter(|segment| segment.contains(column))?,
            ),
            DiffViewMode::Split => {
                if let Some(segment) = selection_line
                    .left
                    .as_ref()
                    .filter(|segment| segment.contains(column))
                {
                    (DiffSelectionPane::Left, segment)
                } else if let Some(segment) = selection_line
                    .right
                    .as_ref()
                    .filter(|segment| segment.contains(column))
                {
                    (DiffSelectionPane::Right, segment)
                } else {
                    return None;
                }
            }
        };

        Some(DiffSelectionPoint {
            display_index,
            pane,
            column: segment.clamp_column(column),
        })
    }

    pub fn selection_point_for_pane(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        display_index: usize,
        pane: DiffSelectionPane,
        column: usize,
    ) -> Option<DiffSelectionPoint> {
        self.ensure_display_cache(mode, width, line_wrap);
        let selection_line = self
            .display_cache
            .entry(mode)
            .selection
            .get(display_index)?;
        let segment = selection_line.segment(pane)?;
        Some(DiffSelectionPoint {
            display_index,
            pane,
            column: segment.clamp_column(column),
        })
    }

    pub fn selected_text(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        anchor: DiffSelectionPoint,
        head: DiffSelectionPoint,
    ) -> Option<String> {
        if anchor.pane != head.pane {
            return None;
        }

        self.ensure_display_cache(mode, width, line_wrap);
        let selection = self.display_cache.entry(mode).selection.as_slice();
        let (start, end) = normalize_selection_points(anchor, head);
        let mut lines = Vec::new();

        for display_index in start.display_index..=end.display_index {
            let segment = selection.get(display_index)?.segment(anchor.pane)?;
            let start_column = if display_index == start.display_index {
                start.column
            } else {
                0
            };
            let end_column = if display_index == end.display_index {
                end.column.saturating_add(1)
            } else {
                segment.content_width
            };
            lines.push(segment.slice(start_column, end_column));
        }

        if lines.iter().all(|line| line.is_empty()) {
            return None;
        }

        Some(lines.join("\n"))
    }

    pub fn selection_columns(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        anchor: DiffSelectionPoint,
        head: DiffSelectionPoint,
        display_index: usize,
    ) -> Option<(usize, usize)> {
        if anchor.pane != head.pane {
            return None;
        }

        self.ensure_display_cache(mode, width, line_wrap);
        let selection = self.display_cache.entry(mode).selection.as_slice();
        let (start, end) = normalize_selection_points(anchor, head);
        if display_index < start.display_index || display_index > end.display_index {
            return None;
        }

        let segment = selection.get(display_index)?.segment(anchor.pane)?;
        let start_column = if display_index == start.display_index {
            start.column
        } else {
            0
        };
        let end_column = if display_index == end.display_index {
            end.column.saturating_add(1)
        } else {
            segment.content_width
        };
        let clamped_start = start_column.min(segment.content_width);
        let clamped_end = end_column.min(segment.content_width);
        (clamped_start < clamped_end).then_some((
            segment.start_column + clamped_start,
            segment.start_column + clamped_end,
        ))
    }

    pub fn first_selectable_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> usize {
        self.nav_targets(mode, width, line_wrap)
            .iter()
            .position(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn last_selectable_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> usize {
        self.nav_targets(mode, width, line_wrap)
            .iter()
            .rposition(|target| target.is_some())
            .unwrap_or(0)
    }

    pub fn move_selection(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        current: usize,
        delta: i32,
    ) -> usize {
        let nav = self.nav_targets(mode, width, line_wrap);
        if nav.is_empty() {
            return 0;
        }

        let mut index = current.min(nav.len().saturating_sub(1));
        if nav[index].is_none() {
            index = nav.iter().position(Option::is_some).unwrap_or(0);
        }

        if delta > 0 {
            for _ in 0..delta {
                let mut probe = index.saturating_add(1);
                while probe < nav.len() && nav[probe].is_none() {
                    probe += 1;
                }
                if probe < nav.len() {
                    index = probe;
                }
            }
        } else if delta < 0 {
            for _ in 0..delta.unsigned_abs() {
                let mut probe = index.saturating_sub(1);
                while probe > 0 && nav[probe].is_none() {
                    probe = probe.saturating_sub(1);
                }
                if nav[probe].is_some() {
                    index = probe;
                }
            }
        }

        index
    }

    pub fn selected_line_number(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        index: usize,
    ) -> Option<usize> {
        match self
            .nav_targets(mode, width, line_wrap)
            .get(index)
            .copied()
            .flatten()
        {
            Some(DisplayNavTarget::Line(line_number)) => Some(line_number),
            _ => None,
        }
    }

    pub fn selected_new_line_number(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        index: usize,
    ) -> Option<usize> {
        self.ensure_display_cache(mode, width, line_wrap);
        let row_refs = self
            .display_cache
            .entry(mode)
            .row_refs
            .get(index)
            .copied()?;
        if let Some(row_index) = row_refs.right {
            return self.rows.get(row_index).and_then(|row| row.new_line);
        }

        if row_refs == DisplayRowRefs::default() {
            return match self
                .display_cache
                .entry(mode)
                .nav
                .get(index)
                .copied()
                .flatten()
            {
                Some(DisplayNavTarget::Line(line_number)) => Some(line_number),
                _ => None,
            };
        }

        None
    }

    pub fn display_index_for_line(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        old_line: Option<usize>,
        new_line: Option<usize>,
    ) -> Option<usize> {
        self.ensure_display_cache(mode, width, line_wrap);
        self.display_cache
            .entry(mode)
            .row_refs
            .iter()
            .position(|row_refs| {
                row_refs
                    .left
                    .and_then(|row_index| self.rows.get(row_index))
                    .is_some_and(|row| row.old_line == old_line && row.new_line == new_line)
                    || row_refs
                        .right
                        .and_then(|row_index| self.rows.get(row_index))
                        .is_some_and(|row| row.old_line == old_line && row.new_line == new_line)
            })
    }

    pub fn display_line_anchor(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        display_index: usize,
    ) -> Option<DiffDisplayLineAnchor> {
        self.ensure_display_cache(mode, width, line_wrap);
        let row_refs = self.display_cache.entry(mode).row_refs.as_slice();
        let anchor = self.anchor_for_display_refs(*row_refs.get(display_index)?)?;
        if display_index > 0
            && row_refs
                .get(display_index - 1)
                .and_then(|previous| self.anchor_for_display_refs(*previous))
                == Some(anchor)
        {
            return None;
        }

        Some(anchor)
    }

    pub fn selected_gap_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        index: usize,
    ) -> Option<usize> {
        self.selected_gap_action(mode, width, line_wrap, index)
            .map(|(gap_index, _)| gap_index)
    }

    pub fn selected_gap_action(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        index: usize,
    ) -> Option<(usize, GapExpandDirection)> {
        match self
            .nav_targets(mode, width, line_wrap)
            .get(index)
            .copied()
            .flatten()
        {
            Some(DisplayNavTarget::Gap(gap_index, direction)) => Some((gap_index, direction)),
            _ => None,
        }
    }

    pub fn selected_conflict_index(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        index: usize,
    ) -> Option<usize> {
        self.ensure_display_cache(mode, width, line_wrap);
        if let Some(Some(DisplayNavTarget::Conflict(conflict_index))) =
            self.display_cache.entry(mode).nav.get(index).copied()
        {
            return Some(conflict_index);
        }

        let row_refs = self
            .display_cache
            .entry(mode)
            .row_refs
            .get(index)
            .copied()?;
        row_refs
            .left
            .and_then(|row_index| self.rows.get(row_index))
            .and_then(|row| row.conflict_index)
            .or_else(|| {
                row_refs
                    .right
                    .and_then(|row_index| self.rows.get(row_index))
                    .and_then(|row| row.conflict_index)
            })
    }

    pub fn display_line_count(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> usize {
        self.nav_targets(mode, width, line_wrap).len()
    }

    pub fn expand_selected_gap(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
        index: usize,
        amount: usize,
    ) -> usize {
        let Some((gap_index, direction)) = self.selected_gap_action(mode, width, line_wrap, index)
        else {
            return index;
        };
        let _ = self.expand_gap(gap_index, direction, amount);
        self.nav_targets(mode, width, line_wrap)
            .iter()
            .position(|target| {
                matches!(
                    target,
                    Some(DisplayNavTarget::Gap(candidate, candidate_direction))
                        if *candidate == gap_index && *candidate_direction == direction
                )
            })
            .unwrap_or(
                index.min(
                    self.nav_targets(mode, width, line_wrap)
                        .len()
                        .saturating_sub(1),
                ),
            )
    }

    fn expand_gap(
        &mut self,
        gap_index: usize,
        direction: GapExpandDirection,
        amount: usize,
    ) -> bool {
        let Some(gap) = self.gaps.iter().find(|gap| gap.gap_index == gap_index) else {
            return false;
        };

        let expansion = self.gap_expansions.entry(gap_index).or_default();
        let remaining = gap
            .new_count
            .saturating_sub(expansion.from_previous + expansion.from_next);
        if remaining == 0 {
            return false;
        }

        let applied = amount.max(1).min(remaining);
        match direction {
            GapExpandDirection::Up => expansion.from_previous += applied,
            GapExpandDirection::Down => expansion.from_next += applied,
        }
        self.invalidate_display_cache();
        true
    }

    fn push_split_window_lines(
        &self,
        row_start: usize,
        row_end: usize,
        width: usize,
        line_wrap: DiffLineWrapMode,
        cursor: &mut usize,
        start: usize,
        end: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        let mut row_index = row_start;

        while row_index < row_end {
            let row = &self.rows[row_index];
            match row.kind {
                DiffLineKind::Removed | DiffLineKind::Added => {
                    let mut removed = Vec::new();
                    let mut added = Vec::new();
                    while row_index < row_end {
                        match self.rows[row_index].kind {
                            DiffLineKind::Removed => removed.push(row_index),
                            DiffLineKind::Added => added.push(row_index),
                            _ => break,
                        }
                        row_index += 1;
                    }

                    let pair_count = removed.len().max(added.len());
                    for pair_index in 0..pair_count {
                        let left = removed
                            .get(pair_index)
                            .and_then(|index| self.rows.get(*index));
                        let right = added
                            .get(pair_index)
                            .and_then(|index| self.rows.get(*index));
                        for rendered_line in
                            render_split_pair_lines(left, right, side_width, line_wrap)
                        {
                            if push_window_line(lines, cursor, start, end, rendered_line.line) {
                                return;
                            }
                        }
                    }
                }
                DiffLineKind::Context => {
                    for rendered_line in
                        render_split_pair_lines(Some(row), Some(row), side_width, line_wrap)
                    {
                        if push_window_line(lines, cursor, start, end, rendered_line.line) {
                            return;
                        }
                    }
                    row_index += 1;
                }
                DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => {
                    for rendered_line in
                        render_unified_code_lines(row, side_width * 2 + 3, line_wrap)
                    {
                        if push_window_line(lines, cursor, start, end, rendered_line.line) {
                            return;
                        }
                    }
                    row_index += 1;
                }
            }
        }
    }

    fn push_gap_window_lines(
        &self,
        gap: &DiffHunkGap,
        width: usize,
        mode: DiffViewMode,
        line_wrap: DiffLineWrapMode,
        cursor: &mut usize,
        start: usize,
        end: usize,
        lines: &mut Vec<Line<'static>>,
    ) {
        let split = matches!(mode, DiffViewMode::Split);
        let gap_width = if split {
            width.saturating_sub(1)
        } else {
            width
        };
        let expansion = self
            .gap_expansions
            .get(&gap.gap_index)
            .copied()
            .unwrap_or_default();
        let context_after_count = expansion.from_previous.min(gap.new_count);
        let remaining_after_previous = gap.new_count.saturating_sub(context_after_count);
        let context_before_count = expansion.from_next.min(remaining_after_previous);

        if let Some(file_lines) = self.new_file_lines.as_ref() {
            let line_start = gap.new_start.saturating_sub(1);
            for offset in 0..context_after_count {
                let line_number = gap.new_start + offset;
                let text = file_lines
                    .get(line_start + offset)
                    .cloned()
                    .unwrap_or_default();
                for rendered_line in render_expanded_context_lines(
                    line_number,
                    &text,
                    self.expanded_context_highlighting(line_number),
                    gap_width,
                    split,
                    line_wrap,
                ) {
                    if push_window_line(lines, cursor, start, end, rendered_line.line) {
                        return;
                    }
                }
            }
        }

        let remaining = gap
            .new_count
            .saturating_sub(context_after_count + context_before_count);
        if remaining > 0 {
            for line in [
                render_expand_gap_line(
                    gap_width,
                    remaining,
                    expansion.from_previous > 0,
                    GapExpandDirection::Up,
                ),
                render_expand_gap_line(
                    gap_width,
                    remaining,
                    expansion.from_next > 0,
                    GapExpandDirection::Down,
                ),
            ] {
                if push_window_line(lines, cursor, start, end, line) {
                    return;
                }
            }
        }

        if let Some(file_lines) = self.new_file_lines.as_ref() {
            let line_start = gap
                .new_start
                .saturating_sub(1)
                .saturating_add(gap.new_count.saturating_sub(context_before_count));
            for offset in 0..context_before_count {
                let line_number = gap.new_start + gap.new_count - context_before_count + offset;
                let text = file_lines
                    .get(line_start + offset)
                    .cloned()
                    .unwrap_or_default();
                for rendered_line in render_expanded_context_lines(
                    line_number,
                    &text,
                    self.expanded_context_highlighting(line_number),
                    gap_width,
                    split,
                    line_wrap,
                ) {
                    if push_window_line(lines, cursor, start, end, rendered_line.line) {
                        return;
                    }
                }
            }
        }
    }

    pub(super) fn ensure_display_cache(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) {
        let cache_is_stale = {
            let cache = self.display_cache.entry(mode);
            !cache.valid || cache.width != width || cache.line_wrap != line_wrap
        };

        if !cache_is_stale {
            return;
        }

        let (lines, nav, row_refs, selection) = if self.rows.is_empty() {
            (
                vec![Line::from(Span::styled(
                    self.note
                        .clone()
                        .unwrap_or_else(|| "No textual diff available.".to_string()),
                    ui::diff_meta_style(),
                ))],
                vec![None],
                vec![DisplayRowRefs::default()],
                vec![DisplaySelectionLine::default()],
            )
        } else {
            match mode {
                DiffViewMode::Unified => self.build_unified_display(width, line_wrap),
                DiffViewMode::Split => self.build_split_display(width, line_wrap),
            }
        };

        let cache = self.display_cache.entry_mut(mode);
        cache.width = width;
        cache.line_wrap = line_wrap;
        cache.lines = lines;
        cache.nav = nav;
        cache.row_refs = row_refs;
        cache.selection = selection;
        cache.valid = true;
    }

    fn nav_targets(
        &mut self,
        mode: DiffViewMode,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> &[Option<DisplayNavTarget>] {
        self.ensure_display_cache(mode, width, line_wrap);
        &self.display_cache.entry(mode).nav
    }

    fn anchor_for_display_refs(&self, row_refs: DisplayRowRefs) -> Option<DiffDisplayLineAnchor> {
        let left = row_refs.left.and_then(|row_index| self.rows.get(row_index));
        let right = row_refs
            .right
            .and_then(|row_index| self.rows.get(row_index));
        let anchor = DiffDisplayLineAnchor {
            old_line: left
                .and_then(|row| row.old_line)
                .or_else(|| right.and_then(|row| row.old_line)),
            new_line: right
                .and_then(|row| row.new_line)
                .or_else(|| left.and_then(|row| row.new_line)),
        };
        (anchor.old_line.is_some() || anchor.new_line.is_some()).then_some(anchor)
    }

    fn build_unified_display(
        &self,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> (
        Vec<Line<'static>>,
        Vec<Option<DisplayNavTarget>>,
        Vec<DisplayRowRefs>,
        Vec<DisplaySelectionLine>,
    ) {
        let mut lines = Vec::new();
        let mut nav = Vec::new();
        let mut row_refs = Vec::new();
        let mut selection = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for row_index in hunk.row_start..hunk.row_end {
                let row = &self.rows[row_index];
                let line_number = match row.kind {
                    DiffLineKind::Added | DiffLineKind::Context => row.new_line,
                    DiffLineKind::Removed => row.old_line,
                    DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => None,
                };
                let display_row_refs = match row.kind {
                    DiffLineKind::Removed => DisplayRowRefs {
                        left: Some(row_index),
                        right: None,
                    },
                    DiffLineKind::Added | DiffLineKind::Context => DisplayRowRefs {
                        left: None,
                        right: Some(row_index),
                    },
                    DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_) => {
                        DisplayRowRefs {
                            left: Some(row_index),
                            right: Some(row_index),
                        }
                    }
                };
                for rendered_line in render_unified_code_lines(row, width, line_wrap) {
                    lines.push(rendered_line.line);
                    nav.push(
                        row.conflict_index
                            .filter(|_| {
                                matches!(
                                    row.kind,
                                    DiffLineKind::ConflictAction | DiffLineKind::ConflictMarker(_)
                                )
                            })
                            .map(DisplayNavTarget::Conflict)
                            .or_else(|| line_number.map(DisplayNavTarget::Line)),
                    );
                    row_refs.push(display_row_refs);
                    selection.push(rendered_line.selection);
                }
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(
                    &mut lines,
                    &mut nav,
                    &mut row_refs,
                    &mut selection,
                    gap,
                    width,
                    false,
                    line_wrap,
                );
            }
        }

        (lines, nav, row_refs, selection)
    }

    fn build_split_display(
        &self,
        width: usize,
        line_wrap: DiffLineWrapMode,
    ) -> (
        Vec<Line<'static>>,
        Vec<Option<DisplayNavTarget>>,
        Vec<DisplayRowRefs>,
        Vec<DisplaySelectionLine>,
    ) {
        let total_width = width.saturating_sub(1);
        let gutter_width = 3;
        let side_width = total_width.saturating_sub(gutter_width) / 2;
        let mut lines = Vec::new();
        let mut nav = Vec::new();
        let mut row_refs = Vec::new();
        let mut selection = Vec::new();

        for (hunk_offset, hunk) in self.hunks.iter().enumerate() {
            for (line, target_line, refs, line_selection) in render_split_hunk_rows(
                &self.rows[hunk.row_start..hunk.row_end],
                hunk.row_start,
                side_width,
                line_wrap,
            ) {
                lines.push(line);
                nav.push(target_line);
                row_refs.push(refs);
                selection.push(line_selection);
            }

            if let Some(gap) = self.gaps.get(hunk_offset) {
                self.push_gap_display_rows(
                    &mut lines,
                    &mut nav,
                    &mut row_refs,
                    &mut selection,
                    gap,
                    total_width,
                    true,
                    line_wrap,
                );
            }
        }

        (lines, nav, row_refs, selection)
    }

    fn push_gap_display_rows(
        &self,
        lines: &mut Vec<Line<'static>>,
        nav: &mut Vec<Option<DisplayNavTarget>>,
        row_refs: &mut Vec<DisplayRowRefs>,
        selection: &mut Vec<DisplaySelectionLine>,
        gap: &DiffHunkGap,
        width: usize,
        split: bool,
        line_wrap: DiffLineWrapMode,
    ) {
        let expansion = self
            .gap_expansions
            .get(&gap.gap_index)
            .copied()
            .unwrap_or_default();
        let context_after_count = expansion.from_previous.min(gap.new_count);
        let remaining_after_previous = gap.new_count.saturating_sub(context_after_count);
        let context_before_count = expansion.from_next.min(remaining_after_previous);

        if let Some(file_lines) = self.new_file_lines.as_ref() {
            let start = gap.new_start.saturating_sub(1);
            for offset in 0..context_after_count {
                let line_number = gap.new_start + offset;
                let text = file_lines.get(start + offset).cloned().unwrap_or_default();
                for rendered_line in render_expanded_context_lines(
                    line_number,
                    &text,
                    self.expanded_context_highlighting(line_number),
                    width,
                    split,
                    line_wrap,
                ) {
                    lines.push(rendered_line.line);
                    nav.push(Some(DisplayNavTarget::Line(line_number)));
                    row_refs.push(DisplayRowRefs::default());
                    selection.push(rendered_line.selection);
                }
            }
        }

        let remaining = gap
            .new_count
            .saturating_sub(context_after_count + context_before_count);
        if remaining > 0 {
            lines.push(render_expand_gap_line(
                width,
                remaining,
                expansion.from_previous > 0,
                GapExpandDirection::Up,
            ));
            nav.push(Some(DisplayNavTarget::Gap(
                gap.gap_index,
                GapExpandDirection::Up,
            )));
            row_refs.push(DisplayRowRefs::default());
            selection.push(DisplaySelectionLine::default());
            lines.push(render_expand_gap_line(
                width,
                remaining,
                expansion.from_next > 0,
                GapExpandDirection::Down,
            ));
            nav.push(Some(DisplayNavTarget::Gap(
                gap.gap_index,
                GapExpandDirection::Down,
            )));
            row_refs.push(DisplayRowRefs::default());
            selection.push(DisplaySelectionLine::default());
        }

        if let Some(file_lines) = self.new_file_lines.as_ref() {
            let start = gap
                .new_start
                .saturating_sub(1)
                .saturating_add(gap.new_count.saturating_sub(context_before_count));
            for offset in 0..context_before_count {
                let line_number = gap.new_start + gap.new_count - context_before_count + offset;
                let text = file_lines.get(start + offset).cloned().unwrap_or_default();
                for rendered_line in render_expanded_context_lines(
                    line_number,
                    &text,
                    self.expanded_context_highlighting(line_number),
                    width,
                    split,
                    line_wrap,
                ) {
                    lines.push(rendered_line.line);
                    nav.push(Some(DisplayNavTarget::Line(line_number)));
                    row_refs.push(DisplayRowRefs::default());
                    selection.push(rendered_line.selection);
                }
            }
        }
    }

    pub(super) fn invalidate_display_cache(&mut self) {
        self.display_cache = DiffDisplayCache::default();
    }

    fn expanded_context_highlighting(&self, line_number: usize) -> Option<Vec<SyntaxToken>> {
        let line_index = line_number.checked_sub(1)?;
        self.new_exact_highlighted_lines
            .as_ref()?
            .get(line_index)
            .cloned()
    }
}

fn push_window_line(
    lines: &mut Vec<Line<'static>>,
    cursor: &mut usize,
    start: usize,
    end: usize,
    line: Line<'static>,
) -> bool {
    if *cursor >= start && *cursor < end {
        lines.push(line);
    }
    *cursor = (*cursor).saturating_add(1);
    *cursor >= end
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DiffDisplayCache {
    unified: CachedDisplay,
    split: CachedDisplay,
}

impl DiffDisplayCache {
    pub(super) fn entry(&self, mode: DiffViewMode) -> &CachedDisplay {
        match mode {
            DiffViewMode::Unified => &self.unified,
            DiffViewMode::Split => &self.split,
        }
    }

    fn entry_mut(&mut self, mode: DiffViewMode) -> &mut CachedDisplay {
        match mode {
            DiffViewMode::Unified => &mut self.unified,
            DiffViewMode::Split => &mut self.split,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CachedDisplay {
    width: usize,
    line_wrap: DiffLineWrapMode,
    pub(super) lines: Vec<Line<'static>>,
    pub(super) nav: Vec<Option<DisplayNavTarget>>,
    pub(super) row_refs: Vec<DisplayRowRefs>,
    pub(super) selection: Vec<DisplaySelectionLine>,
    valid: bool,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DisplaySelectionLine {
    pub(super) unified: Option<DisplaySelectionSegment>,
    pub(super) left: Option<DisplaySelectionSegment>,
    pub(super) right: Option<DisplaySelectionSegment>,
}

impl DisplaySelectionLine {
    fn segment(&self, pane: DiffSelectionPane) -> Option<&DisplaySelectionSegment> {
        match pane {
            DiffSelectionPane::Unified => self.unified.as_ref(),
            DiffSelectionPane::Left => self.left.as_ref(),
            DiffSelectionPane::Right => self.right.as_ref(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DisplaySelectionSegment {
    pub(super) start_column: usize,
    pub(super) content_width: usize,
    pub(super) text: String,
}

impl DisplaySelectionSegment {
    fn contains(&self, column: usize) -> bool {
        column >= self.start_column
            && column < self.start_column.saturating_add(self.content_width)
            && self.content_width > 0
    }

    fn clamp_column(&self, column: usize) -> usize {
        if self.content_width == 0 {
            return 0;
        }

        if column <= self.start_column {
            0
        } else {
            (column - self.start_column).min(self.content_width.saturating_sub(1))
        }
    }

    fn slice(&self, start: usize, end: usize) -> String {
        let text_width = UnicodeWidthStr::width(self.text.as_str());
        if text_width == 0 {
            return String::new();
        }

        let start = start.min(text_width);
        let end = end.min(text_width);
        if start >= end {
            return String::new();
        }

        slice_string_by_width(&self.text, start, end)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum DisplayNavTarget {
    Line(usize),
    Conflict(usize),
    Gap(usize, GapExpandDirection),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DisplayRowRefs {
    pub(super) left: Option<usize>,
    pub(super) right: Option<usize>,
}
