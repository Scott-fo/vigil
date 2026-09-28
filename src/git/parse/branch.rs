use super::super::{
    LOG_FIELD_SEPARATOR,
    branch::{BranchEntry, BranchLocation, BranchTip, Divergence, Upstream},
};

/// `git for-each-ref` format read by [`parse_branch_refs`]. Fields are
/// separated by U+001F; one ref per line.
pub(crate) const BRANCH_REF_FORMAT: &str = "--format=%(HEAD)%1f%(refname)%1f%(refname:short)%1f%(upstream:short)%1f%(upstream:track,nobracket)%1f%(objectname:short)%1f%(committerdate:unix)%1f%(contents:subject)";

/// Parses `for-each-ref` output over `refs/heads` and `refs/remotes`.
/// Symbolic `<remote>/HEAD` refs are skipped. `remotes` resolves which prefix
/// of a remote branch name is the remote, since remote names may contain `/`.
pub(crate) fn parse_branch_refs(raw: &str, remotes: &[String]) -> Vec<BranchEntry> {
    raw.lines()
        .filter_map(|line| parse_branch_ref(line, remotes))
        .collect()
}

fn parse_branch_ref(line: &str, remotes: &[String]) -> Option<BranchEntry> {
    let mut fields = line.split(LOG_FIELD_SEPARATOR);
    let head_marker = fields.next()?;
    let full_ref = fields.next()?;
    let name = fields.next()?.to_string();
    let upstream_name = fields.next()?;
    let track = fields.next()?;
    let short_hash = fields.next()?.to_string();
    let committed_at = fields.next()?.trim().parse().unwrap_or_default();
    let subject = fields.collect::<Vec<_>>().join(" ");

    let location = if full_ref.starts_with("refs/heads/") {
        BranchLocation::Local
    } else if let Some(remote_ref) = full_ref.strip_prefix("refs/remotes/") {
        if remote_ref.ends_with("/HEAD") {
            return None;
        }
        BranchLocation::Remote {
            remote: remote_for(remote_ref, remotes),
        }
    } else {
        return None;
    };

    let upstream = (!upstream_name.is_empty()).then(|| Upstream {
        name: upstream_name.to_string(),
        divergence: parse_divergence(track),
    });

    Some(BranchEntry {
        name,
        is_head: head_marker == "*" && location == BranchLocation::Local,
        location,
        upstream,
        tip: BranchTip {
            short_hash,
            subject,
            committed_at,
        },
    })
}

fn remote_for(remote_ref: &str, remotes: &[String]) -> String {
    remotes
        .iter()
        .filter(|remote| {
            remote_ref
                .strip_prefix(remote.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|remote| remote.len())
        .cloned()
        .unwrap_or_else(|| {
            remote_ref
                .split_once('/')
                .map_or(remote_ref, |(remote, _)| remote)
                .to_string()
        })
}

/// Reads `%(upstream:track,nobracket)`: empty when in sync, `gone`, or
/// `ahead N`, `behind N`, `ahead N, behind M`.
pub(crate) fn parse_divergence(track: &str) -> Divergence {
    let track = track.trim();
    if track == "gone" {
        return Divergence::Gone;
    }

    let mut ahead = 0;
    let mut behind = 0;
    for part in track.split(',') {
        match part.trim().split_once(' ') {
            Some(("ahead", count)) => ahead = count.trim().parse().unwrap_or_default(),
            Some(("behind", count)) => behind = count.trim().parse().unwrap_or_default(),
            _ => {}
        }
    }
    Divergence::Tracking { ahead, behind }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(fields: &[&str]) -> String {
        fields.join(&LOG_FIELD_SEPARATOR.to_string())
    }

    #[test]
    fn divergence_reads_every_track_shape() {
        assert_eq!(
            parse_divergence(""),
            Divergence::Tracking {
                ahead: 0,
                behind: 0
            }
        );
        assert_eq!(parse_divergence("gone"), Divergence::Gone);
        assert_eq!(
            parse_divergence("ahead 3"),
            Divergence::Tracking {
                ahead: 3,
                behind: 0
            }
        );
        assert_eq!(
            parse_divergence("behind 2"),
            Divergence::Tracking {
                ahead: 0,
                behind: 2
            }
        );
        assert_eq!(
            parse_divergence("ahead 1, behind 12"),
            Divergence::Tracking {
                ahead: 1,
                behind: 12
            }
        );
    }

    #[test]
    fn branch_refs_mark_head_upstream_and_remote() {
        let raw = [
            line(&[
                "*",
                "refs/heads/main",
                "main",
                "origin/main",
                "ahead 1",
                "abc1234",
                "1700000000",
                "Add parser",
            ]),
            line(&[
                " ",
                "refs/heads/spike",
                "spike",
                "",
                "",
                "def5678",
                "1690000000",
                "Try things",
            ]),
            line(&[
                " ",
                "refs/remotes/origin/HEAD",
                "origin",
                "",
                "",
                "abc1234",
                "1700000000",
                "Add parser",
            ]),
            line(&[
                " ",
                "refs/remotes/team/eu/feature",
                "team/eu/feature",
                "",
                "",
                "0123456",
                "1600000000",
                "Remote work",
            ]),
        ]
        .join("\n");

        let branches = parse_branch_refs(&raw, &["origin".to_string(), "team/eu".to_string()]);

        assert_eq!(branches.len(), 3);
        assert!(branches[0].is_head);
        assert_eq!(
            branches[0].upstream,
            Some(Upstream {
                name: "origin/main".to_string(),
                divergence: Divergence::Tracking {
                    ahead: 1,
                    behind: 0
                },
            })
        );
        assert_eq!(branches[0].tip.committed_at, 1_700_000_000);
        assert!(!branches[1].is_head);
        assert_eq!(branches[1].upstream, None);
        assert_eq!(
            branches[2].location,
            BranchLocation::Remote {
                remote: "team/eu".to_string()
            }
        );
        assert_eq!(branches[2].local_name(), "feature");
    }
}
