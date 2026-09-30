//! Git remotes and the repositories their URLs name.
//!
//! [`list_remotes`] reads every remote of a checkout from local config: its
//! URL as configured, the URL git actually fetches from after
//! `url.<base>.insteadOf` rewrites, and the default-repository mark the GitHub
//! CLI stores on it. [`parse_remote_url`] reads the host, owner, and name out
//! of any URL form git accepts for a hosted repository.
//!
//! Both are local and cheap: two `git` processes, no network. The listing is
//! a snapshot of the config at call time.

use std::{collections::HashMap, path::Path};

use color_eyre::eyre::eyre;

use super::command::{git_output, git_output_raw};
use crate::forge::RepositoryRef;

/// One configured remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    /// `remote.<name>.url` as written in the config.
    pub url: String,
    /// The URL git fetches from: `url` after `url.<base>.insteadOf`
    /// rewrites, as `git remote -v` shows it.
    pub fetch_url: String,
    /// `remote.<name>.gh-resolved`, which `gh repo set-default` writes to
    /// mark the default repository: `base` for this remote's own
    /// repository, or an explicit `owner/name`.
    pub gh_resolved: Option<String>,
}

/// Every remote with a URL, in config order.
pub async fn list_remotes(repo_root: &Path) -> color_eyre::Result<Vec<Remote>> {
    let (config, fetch_urls) = tokio::join!(
        git_output_raw(
            repo_root,
            &["config", "--get-regexp", r"^remote\..*\.(url|gh-resolved)$"],
        ),
        git_output(repo_root, &["remote", "-v"]),
    );
    let config = config?;
    // `git config --get-regexp` exits 1 when nothing matches: no remotes.
    let config = match config.status.code() {
        Some(0) => String::from_utf8_lossy(&config.stdout).into_owned(),
        Some(1) => String::new(),
        _ => {
            return Err(eyre!(
                "git config failed: {}",
                String::from_utf8_lossy(&config.stderr).trim()
            ));
        }
    };
    Ok(parse_remotes(&config, &fetch_urls?))
}

/// Remotes from `git config --get-regexp '^remote\..*\.(url|gh-resolved)$'`
/// output, with fetch URLs from `git remote -v`.
fn parse_remotes(config: &str, remote_verbose: &str) -> Vec<Remote> {
    let fetch_urls = remote_verbose
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once('\t')?;
            // Partial clones append their filter: `<url> (fetch) [blob:none]`.
            let (url, filter) = rest.rsplit_once(" (fetch)")?;
            let filter = filter.trim();
            if !(filter.is_empty() || filter.starts_with('[') && filter.ends_with(']')) {
                return None;
            }
            Some((name.to_string(), url.trim().to_string()))
        })
        .collect::<HashMap<_, _>>();

    let mut remotes: Vec<Remote> = Vec::new();
    let mut resolved = Vec::new();
    for line in config.lines() {
        let Some((key, value)) = line.split_once(' ') else {
            continue;
        };
        let Some(key) = key.strip_prefix("remote.") else {
            continue;
        };
        let value = value.trim().to_string();
        if let Some(name) = key.strip_suffix(".url") {
            // A remote may list several URLs; git fetches from the first.
            if remotes.iter().any(|remote| remote.name == name) {
                continue;
            }
            remotes.push(Remote {
                name: name.to_string(),
                fetch_url: fetch_urls
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| value.clone()),
                url: value,
                gh_resolved: None,
            });
        } else if let Some(name) = key.strip_suffix(".gh-resolved") {
            resolved.push((name.to_string(), value));
        }
    }
    for (name, value) in resolved {
        if let Some(remote) = remotes.iter_mut().find(|remote| remote.name == name) {
            remote.gh_resolved = Some(value);
        }
    }
    remotes
}

/// The repository a remote URL names, in any of git's URL forms:
/// `[user@]host:owner/name[.git]`, `ssh://[user@]host[:port]/owner/name`,
/// `https://[user@]host[:port]/owner/name[.git]`, or `git://host/owner/name`.
/// The host is lowercased; owner and name keep the URL's spelling.
///
/// `None` for local paths and for paths that are not exactly `owner/name`.
pub fn parse_remote_url(url: &str) -> Option<RepositoryRef> {
    let (host, path) = split_remote_url(url)?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    if host.is_empty() || owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some(RepositoryRef {
        host: host.to_ascii_lowercase(),
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

/// `(host, path)` of a remote URL, with any user and port removed.
fn split_remote_url(url: &str) -> Option<(&str, &str)> {
    let url = url.trim();
    if let Some((_, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?;
        let host = host.split(':').next()?;
        return Some((host, path));
    }
    // scp-like syntax: `[user@]host:path`. A local path has no colon before
    // its first slash.
    let (authority, path) = url.split_once(':')?;
    if authority.contains('/') {
        return None;
    }
    let host = authority.rsplit('@').next()?;
    Some((host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository(host: &str, owner: &str, name: &str) -> RepositoryRef {
        RepositoryRef {
            host: host.to_string(),
            owner: owner.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn remote_urls_parse_in_ssh_https_and_git_forms() {
        let vigil = repository("github.com", "Scott-fo", "vigil");
        for url in [
            "git@github.com:Scott-fo/vigil.git",
            "git@GitHub.com:Scott-fo/vigil",
            "ssh://git@github.com/Scott-fo/vigil.git",
            "ssh://git@github.com:22/Scott-fo/vigil",
            "https://github.com/Scott-fo/vigil",
            "https://github.com/Scott-fo/vigil.git",
            "https://token@github.com/Scott-fo/vigil/",
            "https://github.com:443/Scott-fo/vigil",
            "git://github.com/Scott-fo/vigil.git",
        ] {
            assert_eq!(parse_remote_url(url), Some(vigil.clone()), "{url}");
        }
        assert_eq!(
            parse_remote_url("git@ghe.example.com:team/tool.git"),
            Some(repository("ghe.example.com", "team", "tool"))
        );
    }

    #[test]
    fn remote_urls_without_an_owner_and_name_do_not_parse() {
        for url in [
            "/tmp/remotes/vigil.git",
            "../vigil.git",
            "file:///tmp/remotes/vigil.git",
            "https://gitlab.com/group/subgroup/vigil.git",
            "https://github.com/vigil",
            "git@github.com:",
        ] {
            assert_eq!(parse_remote_url(url), None, "{url}");
        }
    }

    #[test]
    fn remotes_pair_config_with_rewritten_fetch_urls_and_gh_defaults() {
        let remotes = parse_remotes(
            "remote.origin.url git@github.com:me/vigil.git\n\
             remote.upstream.url git@github.com:Scott-fo/vigil.git\n\
             remote.upstream.url https://mirror.example.com/vigil.git\n\
             remote.upstream.gh-resolved base\n\
             remote.gone.gh-resolved base\n",
            "origin\tgit@github.com:me/vigil.git (fetch)\n\
             origin\tgit@github.com:me/vigil.git (push)\n\
             upstream\t/srv/mirror/vigil.git (fetch)\n\
             upstream\tgit@github.com:Scott-fo/vigil.git (push)\n",
        );
        assert_eq!(
            remotes,
            vec![
                Remote {
                    name: "origin".to_string(),
                    url: "git@github.com:me/vigil.git".to_string(),
                    fetch_url: "git@github.com:me/vigil.git".to_string(),
                    gh_resolved: None,
                },
                Remote {
                    name: "upstream".to_string(),
                    url: "git@github.com:Scott-fo/vigil.git".to_string(),
                    fetch_url: "/srv/mirror/vigil.git".to_string(),
                    gh_resolved: Some("base".to_string()),
                },
            ]
        );
    }

    #[test]
    fn partial_clones_keep_their_rewritten_fetch_url() {
        let remotes = parse_remotes(
            "remote.origin.url https://github.com/Scott-fo/vigil.git\n",
            "origin\thttps://mirror.example.com/vigil.git (fetch) [blob:none]\n\
             origin\thttps://github.com/Scott-fo/vigil.git (push)\n",
        );
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].fetch_url, "https://mirror.example.com/vigil.git");
    }
}
