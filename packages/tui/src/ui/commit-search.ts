import fuzzysort from "fuzzysort";
import type { CommitDiffSelection } from "#data/git.ts";

export function searchCommits(
	commits: ReadonlyArray<CommitDiffSelection>,
	query: string,
): ReadonlyArray<CommitDiffSelection> {
	const normalizedQuery = query.trim();
	if (normalizedQuery.length === 0) {
		return commits;
	}

	return fuzzysort
		.go(normalizedQuery, commits, {
			keys: ["shortHash", "commitHash", "subject"],
			limit: commits.length,
		})
		.map((result) => result.obj);
}
