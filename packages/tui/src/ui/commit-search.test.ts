import { describe, expect, test } from "bun:test";
import type { CommitDiffSelection } from "#data/git.ts";
import { searchCommits } from "#ui/commit-search.ts";

function commit(
	commitHash: string,
	shortHash: string,
	subject: string,
): CommitDiffSelection {
	return {
		commitHash,
		baseRef: "main",
		shortHash,
		subject,
	};
}

describe("searchCommits", () => {
	test("returns commits unchanged when query is blank", () => {
		const commits = [
			commit("abc123", "abc123", "fix login form"),
			commit("def456", "def456", "add theme picker"),
		];
		expect(searchCommits(commits, "")).toEqual(commits);
		expect(searchCommits(commits, "   ")).toEqual(commits);
	});

	test("matches by subject and hash", () => {
		const commits = [
			commit("deadbeef11", "deadbeef", "fix login form"),
			commit("cafe000011", "cafe0000", "add settings modal"),
			commit("feedface11", "feedface", "refactor parser"),
		];

		expect(searchCommits(commits, "login")[0]?.shortHash).toBe("deadbeef");
		expect(searchCommits(commits, "cafe")[0]?.shortHash).toBe("cafe0000");
	});
});
