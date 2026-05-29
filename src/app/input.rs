use crossterm::event::{KeyEvent, KeyModifiers};

pub(super) fn is_plain_text_key(key_event: KeyEvent) -> bool {
    !key_event.modifiers.contains(KeyModifiers::CONTROL)
        && !key_event.modifiers.contains(KeyModifiers::ALT)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::is_plain_text_key;

    #[test]
    fn plain_text_keys_reject_control_and_alt_modifiers() {
        assert!(is_plain_text_key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE
        )));
        assert!(!is_plain_text_key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_plain_text_key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::ALT
        )));
    }
}
