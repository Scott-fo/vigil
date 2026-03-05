import { Effect, Option } from "effect";
import type { BlameTarget } from "#tui/types.ts";
import {
	type CommitDiffSelection,
	EMPTY_TREE_HASH,
	GitCommandError,
	runGitEffectAsync,
} from "#data/git/core.ts";

const FIELD_SEPARATOR = "\u001f";
const UNCOMMITTED_BLAME_HASH = "0000000000000000000000000000000000000000";

export interface ParsedBlameHeader {
	readonly commitHash: string;
	readonly author: string;
	readonly date: string;
	readonly summary: string;
}

export interface ParsedCommitShow {
	readonly commitHash: string;
	readonly shortHash: string;
	readonly parentHashes: ReadonlyArray<string>;
	readonly date: string;
	readonly author: string;
	readonly subject: string;
	readonly description: string;
}

export interface BlameCommitDetails {
	readonly target: BlameTarget;
	readonly commitHash: string;
	readonly shortHash: string;
	readonly author: string;
	readonly date: string;
	readonly subject: string;
	readonly description: string;
	readonly isUncommitted: boolean;
	readonly compareSelection: Option.Option<CommitDiffSelection>;
}

export function isUncommittedBlameHash(hash: string): boolean {
	return hash.trim() === UNCOMMITTED_BLAME_HASH;
}

function formatUnixDate(rawSeconds: string): string {
	const seconds = Number(rawSeconds);
	if (!Number.isFinite(seconds) || seconds <= 0) {
		return "";
	}
	const date = new Date(seconds * 1000);
	return Number.isNaN(date.getTime()) ? "" : date.toISOString().slice(0, 10);
}

export function parseBlamePorcelainHeader(
	rawOutput: string,
): Option.Option<ParsedBlameHeader> {
	const lines = rawOutput.split("\n");
	const firstLine = lines[0]?.trim() ?? "";
	const firstLineFields = firstLine.split(/\s+/);
	const commitHash = firstLineFields[0]?.trim() ?? "";
	if (commitHash.length !== 40) {
		return Option.none();
	}

	let author = "";
	let date = "";
	let summary = "";

	for (const line of lines.slice(1)) {
		if (line.startsWith("\t")) {
			break;
		}
		if (line.startsWith("author ")) {
			author = line.slice("author ".length).trim();
			continue;
		}
		if (line.startsWith("author-time ")) {
			date = formatUnixDate(line.slice("author-time ".length).trim());
			continue;
		}
		if (line.startsWith("summary ")) {
			summary = line.slice("summary ".length).trim();
		}
	}

	return Option.some({
		commitHash,
		author,
		date,
		summary,
	});
}

export function parseCommitShowOutput(
	rawOutput: string,
): Option.Option<ParsedCommitShow> {
	const [
		commitHash = "",
		shortHash = "",
		parentsRaw = "",
		date = "",
		author = "",
		subject = "",
		description = "",
	] = rawOutput.split(FIELD_SEPARATOR);

	if (commitHash.length === 0 || shortHash.length === 0) {
		return Option.none();
	}

	return Option.some({
		commitHash,
		shortHash,
		parentHashes: parentsRaw
			.split(" ")
			.map((hash) => hash.trim())
			.filter((hash) => hash.length > 0),
		date: date.trim(),
		author: author.trim(),
		subject: subject.trim(),
		description: description.trim(),
	});
}

function buildParseError(
	args: ReadonlyArray<string>,
	stdout: string,
	stderr: string,
	fallbackMessage: string,
): GitCommandError {
	return new GitCommandError({
		args: [...args],
		stdout,
		stderr,
		fallbackMessage,
	});
}

export function loadBlameCommitDetails(
	target: BlameTarget,
): Effect.Effect<BlameCommitDetails, GitCommandError> {
	const blameArgs = [
		"blame",
		"--porcelain",
		"-L",
		`${target.lineNumber},${target.lineNumber}`,
		"--",
		target.filePath,
	];

	return Effect.gen(function* () {
		const blameResult = yield* runGitEffectAsync(
			blameArgs,
			`Unable to load blame for ${target.filePath}:${target.lineNumber}.`,
		);
		const parsedHeader = parseBlamePorcelainHeader(blameResult.stdout);

		if (Option.isNone(parsedHeader)) {
			return yield* Effect.fail(
				buildParseError(
					blameArgs,
					blameResult.stdout,
					blameResult.stderr,
					`Unable to parse blame output for ${target.filePath}:${target.lineNumber}.`,
				),
			);
		}

		const header = parsedHeader.value;
		if (isUncommittedBlameHash(header.commitHash)) {
			return {
				target,
				commitHash: header.commitHash,
				shortHash: "working-tree",
				author: header.author.length > 0 ? header.author : "Uncommitted",
				date: header.date,
				subject:
					header.summary.length > 0
						? header.summary
						: "Uncommitted line changes",
				description:
					"This line has uncommitted changes. Commit comparison is unavailable.",
				isUncommitted: true,
				compareSelection: Option.none(),
			} satisfies BlameCommitDetails;
		}

		const showArgs = [
			"show",
			"-s",
			"--date=short",
			`--format=%H${FIELD_SEPARATOR}%h${FIELD_SEPARATOR}%P${FIELD_SEPARATOR}%ad${FIELD_SEPARATOR}%an${FIELD_SEPARATOR}%s${FIELD_SEPARATOR}%b`,
			header.commitHash,
		];
		const showResult = yield* runGitEffectAsync(
			showArgs,
			`Unable to load commit metadata for ${header.commitHash}.`,
		);
		const parsedShow = parseCommitShowOutput(showResult.stdout);
		if (Option.isNone(parsedShow)) {
			return yield* Effect.fail(
				buildParseError(
					showArgs,
					showResult.stdout,
					showResult.stderr,
					`Unable to parse commit metadata for ${header.commitHash}.`,
				),
			);
		}

		const commit = parsedShow.value;
		const compareSelection: CommitDiffSelection = {
			commitHash: commit.commitHash,
			baseRef: commit.parentHashes[0] ?? EMPTY_TREE_HASH,
			shortHash: commit.shortHash,
			subject: commit.subject.length > 0 ? commit.subject : header.summary,
		};

		return {
			target,
			commitHash: commit.commitHash,
			shortHash: commit.shortHash,
			author: commit.author.length > 0 ? commit.author : header.author,
			date: commit.date.length > 0 ? commit.date : header.date,
			subject: commit.subject.length > 0 ? commit.subject : header.summary,
			description: commit.description,
			isUncommitted: false,
			compareSelection: Option.some(compareSelection),
		} satisfies BlameCommitDetails;
	});
}
