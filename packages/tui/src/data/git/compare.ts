import { Effect, pipe } from "effect";
import {
	type BranchDiffSelection,
	type CommitDiffSelection,
	type CommitSearchEntry,
	EMPTY_TREE_HASH,
	buildBranchDiffRange,
	type GitCommandError,
	normalizeCommitDiffSelection,
	normalizeBranchDiffSelection,
	runGitEffectAsync,
} from "#data/git/core.ts";
import { parseDiffNameStatusEntries, toFileEntry } from "#data/git/parsers.ts";
import type { FileEntry } from "#tui/types.ts";

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
							(!ref.shortRef.includes("/") || ref.shortRef.endsWith("/HEAD"))
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

		const statusEntries = parseDiffNameStatusEntries(
			statusResult.stdout,
		).filter((entry) => entry.status !== "!!");
		return statusEntries.map(toFileEntry);
	});
}

const LOG_FIELD_SEPARATOR = "\u001f";
const LOG_RECORD_SEPARATOR = "\u001e";

function parseCommitLogEntries(raw: string): ReadonlyArray<CommitSearchEntry> {
	return raw
		.split(LOG_RECORD_SEPARATOR)
		.map((record) => record.trim())
		.filter((record) => record.length > 0)
		.flatMap((record) => {
			const [
				hash = "",
				parentsRaw = "",
				shortHash = "",
				date = "",
				author = "",
				subject = "",
			] = record.split(LOG_FIELD_SEPARATOR);
			if (!hash || !shortHash) {
				return [];
			}
			return [
				{
					hash,
					shortHash,
					parentHashes: parentsRaw
						.split(" ")
						.map((parentHash) => parentHash.trim())
						.filter((parentHash) => parentHash.length > 0),
					author,
					date,
					subject,
				} satisfies CommitSearchEntry,
			];
		});
}

export function listSearchableCommits(
	limit = 12_000,
): Effect.Effect<ReadonlyArray<CommitSearchEntry>, GitCommandError> {
	return pipe(
		runGitEffectAsync(
			[
				"log",
				`--max-count=${Math.max(1, Math.floor(limit))}`,
				"--date=short",
				`--pretty=format:%H%x1f%P%x1f%h%x1f%ad%x1f%an%x1f%s%x1e`,
			],
			"Unable to list commits.",
		),
		Effect.map((result) => parseCommitLogEntries(result.stdout)),
	);
}

export function resolveCommitBaseRef(
	commit: Pick<CommitSearchEntry, "parentHashes">,
): string {
	return commit.parentHashes[0] ?? EMPTY_TREE_HASH;
}

export function loadFilesWithCommitDiff(
	selection: CommitDiffSelection,
): Effect.Effect<FileEntry[], GitCommandError> {
	return Effect.gen(function* () {
		const normalizedSelection = normalizeCommitDiffSelection(selection);
		const statusResult = yield* runGitEffectAsync(
			[
				"diff",
				"--name-status",
				"--find-renames",
				"-z",
				normalizedSelection.baseRef,
				normalizedSelection.commitHash,
			],
			"Unable to load commit comparison file list.",
		);

		const statusEntries = parseDiffNameStatusEntries(
			statusResult.stdout,
		).filter((entry) => entry.status !== "!!");
		return statusEntries.map(toFileEntry);
	});
}
