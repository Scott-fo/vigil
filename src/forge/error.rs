//! Typed failures from the forge adapter and their classification from `gh`
//! output.

use std::fmt;

use serde_json::Value;

/// Why a forge operation failed.
///
/// The variants separate conditions a caller can react to differently: ask
/// the user to install or log in to `gh`, hide PR features for a repository
/// without a GitHub remote, show "not found", back off, or surface GitHub's
/// message verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeError {
    /// The `gh` executable is not on `PATH`.
    GhNotInstalled,
    /// `gh` has no usable token for the host.
    NotAuthenticated { message: String },
    /// The directory is not a git repository with a GitHub remote.
    NotGitHubRepository { message: String },
    /// The repository, pull request, thread, or ref does not exist or is
    /// not visible to the viewer.
    NotFound { message: String },
    /// Primary or secondary rate limit; retry later.
    RateLimited { message: String },
    /// The request was rejected before contacting GitHub.
    InvalidRequest { reason: String },
    /// `gh` failed for another reason. `stderr` includes GitHub's error
    /// messages when the API returned any.
    CommandFailed { args: Vec<String>, stderr: String },
    /// `gh` succeeded but its output did not have the expected shape.
    DecodeFailed { context: String, message: String },
}

impl fmt::Display for ForgeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GhNotInstalled => write!(
                formatter,
                "the GitHub CLI (gh) is not installed; install it to use pull requests"
            ),
            Self::NotAuthenticated { message } => {
                write!(
                    formatter,
                    "gh is not logged in; run `gh auth login` ({message})"
                )
            }
            Self::NotGitHubRepository { message } => {
                write!(formatter, "not a GitHub repository: {message}")
            }
            Self::NotFound { message } => write!(formatter, "not found: {message}"),
            Self::RateLimited { message } => {
                write!(formatter, "GitHub rate limit exceeded: {message}")
            }
            Self::InvalidRequest { reason } => write!(formatter, "{reason}"),
            Self::CommandFailed { args, stderr } => {
                write!(formatter, "gh {} failed: {stderr}", args.join(" "))
            }
            Self::DecodeFailed { context, message } => {
                write!(
                    formatter,
                    "unexpected GitHub response for {context}: {message}"
                )
            }
        }
    }
}

impl std::error::Error for ForgeError {}

impl ForgeError {
    /// GitHub cannot serve this checkout at all: `gh` is not installed or
    /// not logged in (for this host), the checkout has no GitHub remote `gh`
    /// accepts, or the repository itself does not exist or is not visible.
    /// Every request fails this way until the user fixes it, so callers can
    /// stop issuing requests in the background.
    pub fn leaves_forge_unavailable(&self) -> bool {
        match self {
            Self::GhNotInstalled
            | Self::NotAuthenticated { .. }
            | Self::NotGitHubRepository { .. } => true,
            // GraphQL's answer for a missing repository, as opposed to a
            // missing pull request or thread inside it.
            Self::NotFound { message } => message
                .to_ascii_lowercase()
                .contains("could not resolve to a repository"),
            _ => false,
        }
    }

    pub(super) fn decode(context: impl Into<String>, message: impl fmt::Display) -> Self {
        Self::DecodeFailed {
            context: context.into(),
            message: message.to_string(),
        }
    }

    pub(super) fn invalid(reason: impl Into<String>) -> Self {
        Self::InvalidRequest {
            reason: reason.into(),
        }
    }

    /// A gateway error GitHub returns when a large query times out; a read
    /// can safely be retried.
    pub(super) fn is_transient(&self) -> bool {
        match self {
            Self::CommandFailed { stderr, .. } => ["HTTP 502", "HTTP 503", "HTTP 504"]
                .iter()
                .any(|code| stderr.contains(code)),
            _ => false,
        }
    }
}

/// `gh` exits with 4 when authentication is required.
const GH_EXIT_AUTH_REQUIRED: i32 = 4;

/// Classifies a failed `gh` run from its exit code and output.
///
/// `gh api` prints the response body to stdout even on failure and a one-line
/// summary to stderr, so both are consulted: the body carries REST `status`
/// codes and GraphQL error `type`s, which are more reliable than message
/// text.
pub(super) fn classify_failure(
    args: &[String],
    exit_code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> ForgeError {
    let stderr_text = String::from_utf8_lossy(stderr).trim().to_string();
    let api = ApiErrorBody::parse(stdout);
    let summary = stderr_text
        .strip_prefix("gh: ")
        .unwrap_or(&stderr_text)
        .to_string();
    let mut detail = summary.clone();
    for message in &api.messages {
        if !detail.contains(message.as_str()) {
            if !detail.is_empty() {
                detail.push_str("; ");
            }
            detail.push_str(message);
        }
    }
    let haystack = detail.to_ascii_lowercase();
    let mentions = |needles: &[&str]| needles.iter().any(|needle| haystack.contains(needle));

    // Checked before authentication: gh suggests `gh auth login` for
    // remotes on unknown hosts too.
    if mentions(&[
        "none of the git remotes",
        "no git remotes found",
        "not a git repository",
    ]) {
        return ForgeError::NotGitHubRepository { message: detail };
    }
    if exit_code == Some(GH_EXIT_AUTH_REQUIRED)
        || api.status == Some(401)
        || mentions(&["gh auth login", "bad credentials", "http 401"])
    {
        return ForgeError::NotAuthenticated { message: detail };
    }
    if api.has_type("RATE_LIMITED")
        || api.status == Some(429)
        || mentions(&["rate limit", "http 429", "submitted too quickly"])
    {
        return ForgeError::RateLimited { message: detail };
    }
    if api.has_type("NOT_FOUND")
        || api.status == Some(404)
        || mentions(&["could not resolve to", "http 404"])
    {
        return ForgeError::NotFound { message: detail };
    }
    ForgeError::CommandFailed {
        args: args.to_vec(),
        stderr: detail,
    }
}

/// The parts of a REST or GraphQL error body worth classifying on.
#[derive(Debug, Default)]
struct ApiErrorBody {
    status: Option<u16>,
    graphql_types: Vec<String>,
    messages: Vec<String>,
}

impl ApiErrorBody {
    fn parse(stdout: &[u8]) -> Self {
        let Ok(value) = serde_json::from_slice::<Value>(stdout) else {
            return Self::default();
        };
        let mut body = Self {
            status: value.get("status").and_then(|status| match status {
                Value::String(text) => text.parse().ok(),
                Value::Number(number) => number.as_u64().and_then(|n| u16::try_from(n).ok()),
                _ => None,
            }),
            ..Self::default()
        };
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            body.messages.push(message.to_string());
        }
        for error in value
            .get("errors")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match error {
                // REST validation errors are sometimes bare strings.
                Value::String(message) => body.messages.push(message.clone()),
                Value::Object(fields) => {
                    if let Some(kind) = fields.get("type").and_then(Value::as_str) {
                        body.graphql_types.push(kind.to_string());
                    }
                    if let Some(message) = fields.get("message").and_then(Value::as_str) {
                        body.messages.push(message.to_string());
                    }
                }
                _ => {}
            }
        }
        body
    }

    fn has_type(&self, kind: &str) -> bool {
        self.graphql_types.iter().any(|candidate| candidate == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> Vec<String> {
        vec!["api".into(), "graphql".into()]
    }

    fn classify(exit: i32, stdout: &str, stderr: &str) -> ForgeError {
        classify_failure(&args(), Some(exit), stdout.as_bytes(), stderr.as_bytes())
    }

    #[test]
    fn missing_remote_is_not_a_github_repository() {
        assert!(matches!(
            classify(1, "", "no git remotes found\n"),
            ForgeError::NotGitHubRepository { .. }
        ));
        assert!(matches!(
            classify(
                1,
                "",
                "none of the git remotes configured for this repository point to a known GitHub host. To tell gh about a new GitHub host, please use `gh auth login`"
            ),
            ForgeError::NotGitHubRepository { .. }
        ));
        assert!(matches!(
            classify(
                1,
                "",
                "failed to run git: fatal: not a git repository (or any of the parent directories): .git"
            ),
            ForgeError::NotGitHubRepository { .. }
        ));
    }

    #[test]
    fn missing_login_and_bad_token_are_not_authenticated() {
        let no_login = classify(
            4,
            "",
            "To get started with GitHub CLI, please run:  gh auth login\nAlternatively, populate the GH_TOKEN environment variable with a GitHub API authentication token.",
        );
        assert!(matches!(no_login, ForgeError::NotAuthenticated { .. }));

        let bad_token = classify(
            1,
            r#"{"message":"Bad credentials","documentation_url":"https://docs.github.com/rest","status":"401"}"#,
            "gh: Bad credentials (HTTP 401)",
        );
        assert_eq!(
            bad_token,
            ForgeError::NotAuthenticated {
                message: "Bad credentials (HTTP 401)".into()
            }
        );
    }

    #[test]
    fn graphql_and_rest_not_found() {
        let graphql = classify(
            1,
            r#"{"data":{"repository":{"pullRequest":null}},"errors":[{"type":"NOT_FOUND","path":["repository","pullRequest"],"message":"Could not resolve to a PullRequest with the number of 9999."}]}"#,
            "gh: Could not resolve to a PullRequest with the number of 9999.",
        );
        assert_eq!(
            graphql,
            ForgeError::NotFound {
                message: "Could not resolve to a PullRequest with the number of 9999.".into()
            }
        );
        let rest = classify(
            1,
            r#"{"message":"Not Found","documentation_url":"https://docs.github.com/rest/repos/repos#get-a-repository","status":"404"}"#,
            "gh: Not Found (HTTP 404)",
        );
        assert!(matches!(rest, ForgeError::NotFound { .. }));
    }

    #[test]
    fn primary_and_secondary_rate_limits() {
        let graphql = classify(
            1,
            r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded for user ID 1."}]}"#,
            "gh: API rate limit exceeded for user ID 1.",
        );
        assert!(matches!(graphql, ForgeError::RateLimited { .. }));
        let secondary = classify(
            1,
            r#"{"message":"You have exceeded a secondary rate limit. Please wait a few minutes before you try again.","status":"403"}"#,
            "gh: You have exceeded a secondary rate limit. Please wait a few minutes before you try again. (HTTP 403)",
        );
        assert!(matches!(secondary, ForgeError::RateLimited { .. }));
    }

    #[test]
    fn validation_failures_keep_github_messages() {
        let error = classify(
            1,
            r#"{"message":"Unprocessable Entity","errors":["Review Can not approve your own pull request"],"status":"422"}"#,
            "gh: Unprocessable Entity (HTTP 422)",
        );
        assert_eq!(
            error,
            ForgeError::CommandFailed {
                args: args(),
                stderr:
                    "Unprocessable Entity (HTTP 422); Review Can not approve your own pull request"
                        .into()
            }
        );
    }

    #[test]
    fn only_failures_about_the_whole_checkout_leave_the_forge_unavailable() {
        let missing_repository = classify(
            1,
            r#"{"data":{"repository":null},"errors":[{"type":"NOT_FOUND","message":"Could not resolve to a Repository with the name 'acme/gone'."}]}"#,
            "gh: Could not resolve to a Repository with the name 'acme/gone'.",
        );
        let missing_pull_request = classify(
            1,
            r#"{"data":{"repository":{"pullRequest":null}},"errors":[{"type":"NOT_FOUND","message":"Could not resolve to a PullRequest with the number of 99."}]}"#,
            "gh: Could not resolve to a PullRequest with the number of 99.",
        );
        assert!(matches!(missing_repository, ForgeError::NotFound { .. }));
        assert!(missing_repository.leaves_forge_unavailable());
        assert!(matches!(missing_pull_request, ForgeError::NotFound { .. }));
        assert!(!missing_pull_request.leaves_forge_unavailable());

        assert!(ForgeError::GhNotInstalled.leaves_forge_unavailable());
        assert!(
            classify(
                4,
                "",
                "To get started with GitHub CLI, please run:  gh auth login"
            )
            .leaves_forge_unavailable()
        );
        assert!(classify(1, "", "no git remotes found").leaves_forge_unavailable());
        assert!(!classify(1, "", "gh: HTTP 502").leaves_forge_unavailable());
    }

    #[test]
    fn gateway_errors_are_transient() {
        let error = classify(1, "<html>502 Bad Gateway</html>", "gh: HTTP 502");
        assert!(error.is_transient());
        assert!(!classify(1, "", "gh: HTTP 500").is_transient());
    }
}
