//! Multi-line text editing for modals that take prose, such as review
//! comments.
//!
//! [`TextArea`] holds lines and a cursor and applies editing keys; it knows
//! nothing about where the text goes. Enter inserts a newline, so modals
//! bind a separate key to submit. The cursor column counts characters, not
//! bytes or display cells.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Editable text with a cursor. Always holds at least one (possibly empty)
/// line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextArea {
    lines: Vec<String>,
    row: usize,
    column: usize,
}

impl Default for TextArea {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            column: 0,
        }
    }
}

impl TextArea {
    /// `text` with the cursor at its end.
    pub fn from_text(text: &str) -> Self {
        let mut area = Self::default();
        area.insert_str(text);
        area
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The cursor as (line, character column).
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.column)
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Whether the text is empty or only whitespace.
    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|line| line.trim().is_empty())
    }

    /// Replaces everything, leaving the cursor at the end.
    pub fn set_text(&mut self, text: &str) {
        *self = Self::from_text(text);
    }

    /// Inserts `text` at the cursor; `\n` starts a new line and `\r` is
    /// dropped.
    pub fn insert_str(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\n' => self.newline(),
                '\r' => {}
                ch => self.insert_char(ch),
            }
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        let byte = self.byte_index();
        self.lines[self.row].insert(byte, ch);
        self.column += 1;
    }

    pub fn newline(&mut self) {
        let byte = self.byte_index();
        let rest = self.lines[self.row].split_off(byte);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.column = 0;
    }

    pub fn backspace(&mut self) {
        if self.column > 0 {
            self.column -= 1;
            let byte = self.byte_index();
            self.lines[self.row].remove(byte);
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.column = self.line_len();
            self.lines[self.row].push_str(&line);
        }
    }

    pub fn delete(&mut self) {
        if self.column < self.line_len() {
            let byte = self.byte_index();
            self.lines[self.row].remove(byte);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    /// Applies an editing key. Returns false for keys it does not edit with,
    /// which the modal may bind.
    pub fn handle_key(&mut self, key_event: KeyEvent) -> bool {
        let control = key_event.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key_event.modifiers.contains(KeyModifiers::ALT);
        match key_event.code {
            KeyCode::Char(ch) if !control && !alt => self.insert_char(ch),
            KeyCode::Enter if !control && !alt => self.newline(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => self.move_vertically(-1),
            KeyCode::Down => self.move_vertically(1),
            KeyCode::Home => self.column = 0,
            KeyCode::End => self.column = self.line_len(),
            KeyCode::Char('a') if control => self.column = 0,
            _ => return false,
        }
        true
    }

    fn move_left(&mut self) {
        if self.column > 0 {
            self.column -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.column = self.line_len();
        }
    }

    fn move_right(&mut self) {
        if self.column < self.line_len() {
            self.column += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.column = 0;
        }
    }

    fn move_vertically(&mut self, delta: isize) {
        let Some(row) = self.row.checked_add_signed(delta) else {
            self.column = 0;
            return;
        };
        if row >= self.lines.len() {
            self.column = self.line_len();
            return;
        }
        self.row = row;
        self.column = self.column.min(self.line_len());
    }

    fn line_len(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    fn byte_index(&self) -> usize {
        let line = &self.lines[self.row];
        line.char_indices()
            .nth(self.column)
            .map_or(line.len(), |(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(area: &mut TextArea, text: &str) {
        for ch in text.chars() {
            let code = if ch == '\n' {
                KeyCode::Enter
            } else {
                KeyCode::Char(ch)
            };
            assert!(area.handle_key(key(code)));
        }
    }

    #[test]
    fn enter_inserts_newlines_and_backspace_joins_lines() {
        let mut area = TextArea::default();
        type_text(&mut area, "first\nsecond");
        assert_eq!(area.lines(), ["first", "second"]);
        assert_eq!(area.cursor(), (1, 6));

        for _ in 0..7 {
            area.handle_key(key(KeyCode::Backspace));
        }
        assert_eq!(area.text(), "first");
        assert_eq!(area.cursor(), (0, 5));
    }

    #[test]
    fn cursor_moves_across_lines_and_edits_in_the_middle() {
        let mut area = TextArea::from_text("héllo\nab");
        area.handle_key(key(KeyCode::Up));
        assert_eq!(area.cursor(), (0, 2));
        type_text(&mut area, "X");
        assert_eq!(area.lines()[0], "héXllo");
        area.handle_key(key(KeyCode::End));
        area.handle_key(key(KeyCode::Delete));
        assert_eq!(area.lines(), ["héXlloab"]);
        area.handle_key(key(KeyCode::Home));
        area.handle_key(key(KeyCode::Left));
        assert_eq!(area.cursor(), (0, 0), "stays at the start");
    }

    #[test]
    fn modal_keys_are_left_to_the_modal() {
        let mut area = TextArea::default();
        assert!(!area.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)));
        assert!(!area.handle_key(key(KeyCode::Esc)));
        assert!(!area.handle_key(key(KeyCode::Tab)));
        assert!(area.is_blank());
    }

    #[test]
    fn pasted_text_keeps_its_lines() {
        let mut area = TextArea::default();
        area.insert_str("a\r\nb\n");
        assert_eq!(area.lines(), ["a", "b", ""]);
        assert!(!area.is_blank());
    }
}
