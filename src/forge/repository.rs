//! Which GitHub repository a checkout belongs to, decided from local git
//! config.
//!
//! `gh` picks the repository behind a checkout from its remotes: a default
//! recorded by `gh repo set-default` (`remote.<name>.gh-resolved`) wins;
//! otherwise, without a terminal to prompt on, it takes the first remote
//! ranked `upstream`, then `github`, then `origin`, then any other name.
//! [`resolve_repository_locally`] repeats that choice without running `gh`,
//! so connecting costs two local `git` processes instead of an API round
//! trip.
//!
//! It answers only when the answer is certain to match `gh`'s: the chosen
//! remote is on `github.com`, and no tie, unknown host, or environment
//! override (`GH_REPO`, `GH_HOST`) could make `gh` choose differently. In
//! every other case, including GitHub Enterprise hosts and ssh host aliases,
//! it returns `None` and the caller asks `gh`. A wrong repository is far
//! worse than a slow connect.

use std::path::Path;

use crate::git::{self, Remote};

use super::types::RepositoryRef;

/// The only host resolved locally. `gh` may know other hosts (GitHub
/// Enterprise) or map ssh aliases to hosts; only `gh` can say.
const GITHUB_HOST: &str = "github.com";

/// Environment variables that change how `gh` picks the repository.
const OVERRIDE_VARIABLES: &[&str] = &["GH_REPO", "GH_HOST"];

/// The repository `gh` would pick for `repo_root`, when local config decides
/// it unambiguously. See the module docs.
pub(super) async fn resolve_repository_locally(repo_root: &Path) -> Option<RepositoryRef> {
    let overridden = OVERRIDE_VARIABLES
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));
    if overridden {
        return None;
    }
    let remotes = git::list_remotes(repo_root).await.ok()?;
    choose_repository(&remotes)
}

/// The repository `gh` picks from `remotes`, or `None` when that is not
/// certain.
fn choose_repository(remotes: &[Remote]) -> Option<RepositoryRef> {
    let mut resolved = remotes.iter().filter(|remote| remote.gh_resolved.is_some());
    if let Some(remote) = resolved.next() {
        // `gh` honors the first mark in its own remote order, and reads a
        // dotted remote name as its first segment; both are ambiguous here.
        if resolved.next().is_some() || remote.name.contains('.') {
            return None;
        }
        let own = on_github_com(git::parse_remote_url(&remote.fetch_url)?)?;
        let mark = remote.gh_resolved.as_deref()?.trim();
        if mark == "base" {
            return Some(own);
        }
        let (owner, name) = mark.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        return Some(RepositoryRef {
            host: own.host,
            owner: owner.to_string(),
            name: name.to_string(),
        });
    }

    let top_rank = remotes.iter().map(rank).max()?;
    let mut top = remotes.iter().filter(|remote| rank(remote) == top_rank);
    let remote = top.next()?;
    if top.next().is_some() {
        return None;
    }
    on_github_com(git::parse_remote_url(&remote.fetch_url)?)
}

/// `gh`'s remote preference: higher ranks first.
fn rank(remote: &Remote) -> u8 {
    match remote.name.as_str() {
        "upstream" => 3,
        "github" => 2,
        "origin" => 1,
        _ => 0,
    }
}

fn on_github_com(repository: RepositoryRef) -> Option<RepositoryRef> {
    (repository.host == GITHUB_HOST).then_some(repository)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(name: &str, url: &str) -> Remote {
        Remote {
            name: name.to_string(),
            url: url.to_string(),
            fetch_url: url.to_string(),
            gh_resolved: None,
        }
    }

    fn resolved(name: &str, url: &str, mark: &str) -> Remote {
        Remote {
            gh_resolved: Some(mark.to_string()),
            ..remote(name, url)
        }
    }

    fn github(owner: &str, name: &str) -> Option<RepositoryRef> {
        Some(RepositoryRef {
            host: "github.com".to_string(),
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }

    #[test]
    fn a_single_github_remote_resolves_in_any_url_form() {
        for url in [
            "git@github.com:Scott-fo/vigil.git",
            "ssh://git@github.com/Scott-fo/vigil",
            "https://github.com/Scott-fo/vigil.git",
        ] {
            assert_eq!(
                choose_repository(&[remote("origin", url)]),
                github("Scott-fo", "vigil"),
                "{url}"
            );
        }
        assert_eq!(
            choose_repository(&[remote("fork", "git@github.com:me/vigil.git")]),
            github("me", "vigil")
        );
    }

    #[test]
    fn upstream_beats_github_beats_origin_beats_other_names() {
        let remotes = [
            remote("aardvark", "git@github.com:a/vigil.git"),
            remote("origin", "git@github.com:me/vigil.git"),
            remote("github", "git@github.com:gh/vigil.git"),
            remote("upstream", "git@github.com:Scott-fo/vigil.git"),
        ];
        assert_eq!(choose_repository(&remotes), github("Scott-fo", "vigil"));
        assert_eq!(choose_repository(&remotes[..3]), github("gh", "vigil"));
        assert_eq!(choose_repository(&remotes[..2]), github("me", "vigil"));
    }

    #[test]
    fn a_gh_default_wins_over_remote_names() {
        let remotes = [
            remote("upstream", "git@github.com:Scott-fo/vigil.git"),
            resolved("fork", "git@github.com:me/vigil.git", "base"),
        ];
        assert_eq!(choose_repository(&remotes), github("me", "vigil"));

        let explicit = [
            remote("upstream", "git@github.com:Scott-fo/vigil.git"),
            resolved("origin", "git@github.com:me/vigil.git", "parent/vigil"),
        ];
        assert_eq!(choose_repository(&explicit), github("parent", "vigil"));
    }

    #[test]
    fn ambiguous_or_malformed_defaults_defer_to_gh() {
        let twice = [
            resolved("origin", "git@github.com:me/vigil.git", "base"),
            resolved("upstream", "git@github.com:Scott-fo/vigil.git", "base"),
        ];
        assert_eq!(choose_repository(&twice), None);

        let dotted = [resolved("my.fork", "git@github.com:me/vigil.git", "base")];
        assert_eq!(choose_repository(&dotted), None);

        for mark in ["", "vigil", "a/b/c", "/vigil"] {
            let remotes = [resolved("origin", "git@github.com:me/vigil.git", mark)];
            assert_eq!(choose_repository(&remotes), None, "{mark:?}");
        }
    }

    #[test]
    fn hosts_other_than_github_com_defer_to_gh() {
        for url in [
            "git@ghe.example.com:team/vigil.git",
            "git@github-work:Scott-fo/vigil.git",
            "https://gitlab.com/Scott-fo/vigil.git",
            "/srv/git/vigil.git",
        ] {
            assert_eq!(choose_repository(&[remote("origin", url)]), None, "{url}");
        }
        // `gh` might skip an unknown host or might know it; either way the
        // github.com remote below it is not certain.
        let unknown_above = [
            remote("origin", "git@github.com:me/vigil.git"),
            remote("upstream", "git@ghe.example.com:team/vigil.git"),
        ];
        assert_eq!(choose_repository(&unknown_above), None);

        let enterprise_default = [resolved("origin", "git@ghe.example.com:t/v.git", "base")];
        assert_eq!(choose_repository(&enterprise_default), None);
    }

    #[test]
    fn ties_and_empty_remote_lists_defer_to_gh() {
        let tie = [
            remote("alpha", "git@github.com:a/vigil.git"),
            remote("zeta", "git@github.com:z/vigil.git"),
        ];
        assert_eq!(choose_repository(&tie), None);
        assert_eq!(choose_repository(&[]), None);
    }

    #[test]
    fn the_rewritten_fetch_url_decides_as_it_does_for_gh() {
        let mut rewritten = remote("origin", "git@github.com:Scott-fo/vigil.git");
        rewritten.fetch_url = "git@ghe.example.com:mirror/vigil.git".to_string();
        assert_eq!(choose_repository(&[rewritten]), None);
    }
}
