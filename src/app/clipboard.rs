use std::io::{Write, stdout};

use super::{ActivePane, App};

impl App {
    pub(super) fn copy_diff_selection_to_clipboard(&mut self) -> color_eyre::Result<bool> {
        if self.active_pane != ActivePane::Diff {
            return Ok(false);
        }

        let Some(selection) = self.diff_text_selection else {
            return Ok(false);
        };
        let text = self.diff_view.selected_text(
            self.diff_view_mode,
            self.current_diff_display_width(),
            selection.anchor,
            selection.head,
        );
        let Some(text) = text else {
            self.status_message = Some("selection is empty".to_string());
            return Ok(true);
        };

        write_osc52_clipboard(&text)?;
        self.status_message = Some("copied diff selection".to_string());
        Ok(true)
    }
}

fn write_osc52_clipboard(text: &str) -> color_eyre::Result<()> {
    let encoded = encode_base64(text.as_bytes());
    let mut output = stdout();
    write!(output, "\x1b]52;c;{encoded}\x07")?;
    output.flush()?;
    Ok(())
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);

        encoded.push(TABLE[(first >> 2) as usize] as char);
        encoded.push(TABLE[((first & 0b0000_0011) << 4 | (second >> 4)) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(TABLE[((second & 0b0000_1111) << 2 | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(TABLE[(third & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}
