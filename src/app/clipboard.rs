use std::io::{Write, stdout};

use crate::review::{ReviewFinding, ReviewSeverity, ReviewVerdict};

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
            self.diff_line_wrap_mode,
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

    pub(super) fn copy_review_summary_to_clipboard(&mut self) -> color_eyre::Result<bool> {
        let Some(text) = self.review_clipboard_text() else {
            self.status_message = Some("no Codex review loaded".to_string());
            return Ok(false);
        };

        write_osc52_clipboard(&text)?;
        self.status_message = Some("copied Codex review summary".to_string());
        Ok(true)
    }

    fn review_clipboard_text(&self) -> Option<String> {
        if let Some(error) = self.review_error.as_deref() {
            return Some(format!("Codex review failed\n\n{error}"));
        }

        let report = self.review_report.as_ref()?;
        let mut text = String::new();
        text.push_str(&report.summary.headline);
        text.push('\n');
        text.push_str(&format!(
            "Verdict: {} | {} comment{}\n\n",
            verdict_label(report.summary.verdict),
            report.findings.len(),
            if report.findings.len() == 1 { "" } else { "s" }
        ));
        text.push_str(&report.summary.body);
        text.push('\n');

        if !report.summary.risk_areas.is_empty() {
            text.push_str("\nRisk areas\n");
            for area in &report.summary.risk_areas {
                text.push_str("- ");
                text.push_str(area);
                text.push('\n');
            }
        }

        if !report.findings.is_empty() {
            text.push_str("\nComments\n");
            for finding in &report.findings {
                text.push_str("- ");
                text.push_str(severity_label(finding.severity));
                text.push_str(": ");
                text.push_str(&finding.title);
                text.push_str(" (");
                text.push_str(&finding_location(finding));
                text.push_str(")\n  ");
                text.push_str(&finding.body);
                text.push('\n');
            }
        }

        Some(text)
    }
}

pub(super) fn write_osc52_clipboard(text: &str) -> color_eyre::Result<()> {
    let encoded = encode_base64(text.as_bytes());
    let mut output = stdout();
    write!(output, "\x1b]52;c;{encoded}\x07")?;
    output.flush()?;
    Ok(())
}

fn finding_location(finding: &ReviewFinding) -> String {
    match (finding.line, finding.end_line) {
        (Some(line), Some(end_line)) if end_line != line => {
            format!("{}:{}-{}", finding.path, line, end_line)
        }
        (Some(line), _) => format!("{}:{}", finding.path, line),
        _ => finding.path.clone(),
    }
}

fn verdict_label(verdict: ReviewVerdict) -> &'static str {
    match verdict {
        ReviewVerdict::Clean => "clean",
        ReviewVerdict::HasConcerns => "has concerns",
        ReviewVerdict::NeedsWork => "needs work",
    }
}

fn severity_label(severity: ReviewSeverity) -> &'static str {
    match severity {
        ReviewSeverity::Critical => "critical",
        ReviewSeverity::High => "high",
        ReviewSeverity::Medium => "medium",
        ReviewSeverity::Low => "low",
        ReviewSeverity::Info => "info",
    }
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
