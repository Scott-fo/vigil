use crate::git::DiffDisplayLineAnchor;

use super::{ReviewFinding, ReviewSeverity, ReviewSide};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDisplayComment {
    pub severity: ReviewSeverity,
    pub title: String,
    pub body: String,
}

pub fn comments_for_display_line(
    findings: &[ReviewFinding],
    path: &str,
    anchor: DiffDisplayLineAnchor,
) -> Vec<ReviewDisplayComment> {
    findings
        .iter()
        .filter(|finding| finding.path == path)
        .filter(|finding| finding_matches_anchor(finding, anchor))
        .map(|finding| ReviewDisplayComment {
            severity: finding.severity,
            title: finding.title.clone(),
            body: finding.body.clone(),
        })
        .collect()
}

fn finding_matches_anchor(finding: &ReviewFinding, anchor: DiffDisplayLineAnchor) -> bool {
    let Some(line) = finding.line.map(|line| line as usize) else {
        return false;
    };

    match finding.side {
        ReviewSide::Old => anchor.old_line == Some(line),
        ReviewSide::New => anchor.new_line == Some(line),
        ReviewSide::Both => anchor.old_line == Some(line) || anchor.new_line == Some(line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{ReviewFindingState, ReviewSeverity};

    fn finding(side: ReviewSide, line: u32) -> ReviewFinding {
        ReviewFinding {
            path: "src/lib.rs".to_string(),
            side,
            line: Some(line),
            end_line: None,
            severity: ReviewSeverity::High,
            title: "Careful".to_string(),
            body: "This line matters.".to_string(),
            suggested_patch: None,
            state: ReviewFindingState::Open,
            fingerprint: String::new(),
        }
    }

    #[test]
    fn maps_new_side_comments_to_new_diff_lines() {
        let comments = comments_for_display_line(
            &[finding(ReviewSide::New, 9)],
            "src/lib.rs",
            DiffDisplayLineAnchor {
                old_line: None,
                new_line: Some(9),
            },
        );

        assert_eq!(comments.len(), 1);
    }

    #[test]
    fn maps_range_comments_only_to_start_line() {
        let mut finding = finding(ReviewSide::New, 9);
        finding.end_line = Some(12);

        let start_comments = comments_for_display_line(
            &[finding.clone()],
            "src/lib.rs",
            DiffDisplayLineAnchor {
                old_line: None,
                new_line: Some(9),
            },
        );
        let middle_comments = comments_for_display_line(
            &[finding],
            "src/lib.rs",
            DiffDisplayLineAnchor {
                old_line: None,
                new_line: Some(10),
            },
        );

        assert_eq!(start_comments.len(), 1);
        assert!(middle_comments.is_empty());
    }
}
