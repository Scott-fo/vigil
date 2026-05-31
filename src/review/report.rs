use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewReport {
    pub summary: ReviewSummary,
    #[serde(default)]
    pub findings: Vec<ReviewFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub headline: String,
    pub verdict: ReviewVerdict,
    pub body: String,
    #[serde(default)]
    pub risk_areas: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewVerdict {
    Clean,
    HasConcerns,
    NeedsWork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFinding {
    pub path: String,
    pub side: ReviewSide,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub severity: ReviewSeverity,
    pub title: String,
    pub body: String,
    pub suggested_patch: Option<String>,
    #[serde(default)]
    pub state: ReviewFindingState,
    #[serde(default)]
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewSide {
    Old,
    New,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewFindingState {
    Open,
    Resolved,
    Stale,
}

impl Default for ReviewFindingState {
    fn default() -> Self {
        Self::Open
    }
}

pub fn parse_review_report(raw: &str) -> color_eyre::Result<ReviewReport> {
    serde_json::from_str(raw.trim()).map_err(Into::into)
}

pub fn review_report_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "findings"],
        "properties": {
            "summary": {
                "type": "object",
                "additionalProperties": false,
                "required": ["headline", "verdict", "body", "riskAreas"],
                "properties": {
                    "headline": { "type": "string" },
                    "verdict": {
                        "type": "string",
                        "enum": ["clean", "hasConcerns", "needsWork"]
                    },
                    "body": { "type": "string" },
                    "riskAreas": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": [
                        "path",
                        "side",
                        "line",
                        "endLine",
                        "severity",
                        "title",
                        "body",
                        "suggestedPatch"
                    ],
                    "properties": {
                        "path": { "type": "string" },
                        "side": {
                            "type": "string",
                            "enum": ["old", "new", "both"]
                        },
                        "line": {
                            "type": ["integer", "null"],
                            "minimum": 1
                        },
                        "endLine": {
                            "type": ["integer", "null"],
                            "minimum": 1
                        },
                        "severity": {
                            "type": "string",
                            "enum": ["info", "low", "medium", "high", "critical"]
                        },
                        "title": { "type": "string" },
                        "body": { "type": "string" },
                        "suggestedPatch": { "type": ["string", "null"] }
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_review_report() {
        let report = parse_review_report(
            r#"{
              "summary": {
                "headline": "Looks close",
                "verdict": "hasConcerns",
                "body": "One risky edge case.",
                "riskAreas": ["state"]
              },
              "findings": [{
                "path": "src/app.rs",
                "side": "new",
                "line": 42,
                "endLine": null,
                "severity": "medium",
                "title": "Check stale requests",
                "body": "The handler can apply old data.",
                "suggestedPatch": null
              }]
            }"#,
        )
        .expect("valid report");

        assert_eq!(report.summary.verdict, ReviewVerdict::HasConcerns);
        assert_eq!(report.findings[0].side, ReviewSide::New);
        assert_eq!(report.findings[0].state, ReviewFindingState::Open);
    }
}
