import { Effect, pipe } from "effect";
import type { FileEntry } from "#tui/types";
import {
	buildBranchDiffRange,
	type BranchDiffSelection,
	type GitCommandError,
	normalizeBranchDiffSelection,
	runGitEffectAsync,
} from "#data/git/core";
import { parseDiffNameStatusEntries, toFileEntry } from "#data/git/parsers";

export function listComparableRefs(): Effect.Effect<
	readonly string[],
	GitCommandError
> {
	return pipe(
		runGitEffectAsync(
			[
				"for-each-ref",
				"--format=%(refname)\t%(refname:short)",
				"refs/heads",
				"refs/remotes",
			],
			"Unable to list git refs.",
		),
		Effect.map((result) => {
			const refs = result.stdout
				.split("\n")
				.map((line) => line.trim())
				.map((line) => {
					const [fullRef = "", shortRef = ""] = line.split("\t");
					return {
						fullRef,
						shortRef,
					};
				})
				.filter(
					(ref) =>
						ref.shortRef.length > 0 &&
						ref.shortRef !== "HEAD" &&
						!(
							ref.fullRef.startsWith("refs/remotes/") &&
							(!ref.shortRef.includes("/") ||
								ref.shortRef.endsWith("/HEAD"))
						),
				)
				.map((ref) => ref.shortRef);
			return [...new Set(refs)].sort((left, right) =>
				left.localeCompare(right),
			);
		}),
	);
}

export function loadFilesWithBranchDiffs(
	selection: BranchDiffSelection,
): Effect.Effect<FileEntry[], GitCommandError> {
	return Effect.gen(function* () {
		const normalizedSelection = normalizeBranchDiffSelection(selection);
		const statusResult = yield* runGitEffectAsync(
			[
				"diff",
				"--name-status",
				"--find-renames",
				"-z",
				buildBranchDiffRange(normalizedSelection),
			],
			"Unable to load branch comparison file list.",
		);

		const statusEntries = parseDiffNameStatusEntries(statusResult.stdout).filter(
			(entry) => entry.status !== "!!",
		);
		return statusEntries.map(toFileEntry);
	});
}
