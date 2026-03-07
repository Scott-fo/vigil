import * as FileSystem from "@effect/platform/FileSystem";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import { Effect, Option, pipe } from "effect";
import {
	type BranchDiffSelection,
	type CommitDiffSelection,
	buildBranchDiffRange,
	normalizeCommitDiffSelection,
	normalizeBranchDiffSelection,
	renderGitCommandError,
	runGitEffectAsync,
} from "#data/git/core.ts";
import type { FileEntry } from "#tui/types.ts";

const TEXT_DECODER = new TextDecoder();

interface FilePreview {
	readonly diff: string;
	readonly note: Option.Option<string>;
}

export type FileDiffPreview = FilePreview;
export type FileDiffContextLines = ReadonlyArray<string>;

function normalizeTextLines(content: string): FileDiffContextLines {
	const normalized = content.replace(/\r\n/g, "\n");
	if (normalized.length === 0) {
		return [];
	}

	const lines = normalized.split("\n");
	if (normalized.endsWith("\n")) {
		lines.pop();
	}

	return lines;
}

function createUntrackedFileDiff(inputPath: string, content: string): string {
	const normalized = content.replace(/\r\n/g, "\n");
	if (normalized.length === 0) {
		return "";
	}

	const hasTrailingNewline = normalized.endsWith("\n");
	const lines = normalized.split("\n");

	if (hasTrailingNewline) {
		lines.pop();
	}

	const lineCount = lines.length;
	const hunkHeader = `@@ -0,0 +1,${lineCount} @@`;
	let body = lines.map((line) => `+${line}`).join("\n");

	if (lineCount > 0 && hasTrailingNewline) {
		body += "\n";
	}

	return [
		`diff --git a/${inputPath} b/${inputPath}`,
		"new file mode 100644",
		"index 0000000..1111111",
		"--- /dev/null",
		`+++ b/${inputPath}`,
		hunkHeader,
		body,
		"",
	].join("\n");
}

function withDefaultPreviewNote(preview: FilePreview): FilePreview {
	return !preview.diff.trim() && Option.isNone(preview.note)
		? {
				diff: preview.diff,
				note: Option.some("No textual diff available."),
			}
		: preview;
}

function loadUntrackedPreview(
	filePath: string,
): Effect.Effect<FilePreview, never> {
	return pipe(
		Effect.gen(function* () {
			const fs = yield* FileSystem.FileSystem;
			return yield* fs.readFile(filePath);
		}),
		Effect.provide(BunFileSystem.layer),
		Effect.match({
			onFailure: () =>
				({
					diff: "",
					note: Option.some("Unable to read untracked file content."),
				}) as const,
			onSuccess: (bytes) => {
				if (bytes.includes(0)) {
					return {
						diff: "",
						note: Option.some("Binary or non-text file; no preview available."),
					} as const;
				}
				const diff = createUntrackedFileDiff(
					filePath,
					TEXT_DECODER.decode(bytes),
				);
				return !diff.trim()
					? ({
							diff,
							note: Option.some(
								"Untracked empty file; no textual hunk to preview.",
							),
						} as const)
					: ({
							diff,
							note: Option.none<string>(),
						} as const);
			},
		}),
	);
}

function loadTrackedPreview(
	filePath: string,
): Effect.Effect<FilePreview, never> {
	return pipe(
		runGitEffectAsync(
			["diff", "--no-color", "--find-renames", "HEAD", "--", filePath],
			"Unable to load diff for this file.",
		),
		Effect.match({
			onFailure: (error) => ({
				diff: "",
				note: Option.some(renderGitCommandError(error)),
			}),
			onSuccess: (result) => ({
				diff: result.stdout,
				note: Option.none<string>(),
			}),
		}),
	);
}

function loadTextFileLines(
	filePath: string,
): Effect.Effect<FileDiffContextLines, never> {
	return pipe(
		Effect.gen(function* () {
			const fs = yield* FileSystem.FileSystem;
			return yield* fs.readFile(filePath);
		}),
		Effect.provide(BunFileSystem.layer),
		Effect.match({
			onFailure: () => [] as FileDiffContextLines,
			onSuccess: (bytes) =>
				bytes.includes(0) ? ([] as FileDiffContextLines) : normalizeTextLines(TEXT_DECODER.decode(bytes)),
		}),
	);
}

function loadRevisionFileLines(
	revision: string,
	filePath: string,
): Effect.Effect<FileDiffContextLines, never> {
	return pipe(
		runGitEffectAsync(
			["show", `${revision}:${filePath}`],
			`Unable to load ${filePath} from ${revision}.`,
		),
		Effect.match({
			onFailure: () => [] as FileDiffContextLines,
			onSuccess: (result) => normalizeTextLines(result.stdout),
		}),
	);
}

function resolveBranchDiffBaseRef(
	selection: BranchDiffSelection,
): Effect.Effect<string, never> {
	return pipe(
		runGitEffectAsync(
			["merge-base", selection.destinationRef, selection.sourceRef],
			"Unable to resolve branch diff base ref.",
		),
		Effect.match({
			onFailure: () => "",
			onSuccess: (result) => result.stdout.trim(),
		}),
	);
}

function loadBranchPreview(
	filePath: string,
	selection: BranchDiffSelection,
): Effect.Effect<FilePreview, never> {
	return pipe(
		runGitEffectAsync(
			[
				"diff",
				"--no-color",
				"--find-renames",
				buildBranchDiffRange(selection),
				"--",
				filePath,
			],
			`Unable to load branch diff for ${filePath}.`,
		),
		Effect.match({
			onFailure: (error) => ({
				diff: "",
				note: Option.some(renderGitCommandError(error)),
			}),
			onSuccess: (result) => ({
				diff: result.stdout,
				note: Option.none<string>(),
			}),
		}),
	);
}

function loadCommitPreview(
	filePath: string,
	selection: CommitDiffSelection,
): Effect.Effect<FilePreview, never> {
	return pipe(
		runGitEffectAsync(
			[
				"diff",
				"--no-color",
				"--find-renames",
				selection.baseRef,
				selection.commitHash,
				"--",
				filePath,
			],
			`Unable to load commit diff for ${filePath}.`,
		),
		Effect.match({
			onFailure: (error) => ({
				diff: "",
				note: Option.some(renderGitCommandError(error)),
			}),
			onSuccess: (result) => ({
				diff: result.stdout,
				note: Option.none<string>(),
			}),
		}),
	);
}

export function loadFilePreview(
	file: Pick<FileEntry, "path" | "status">,
): Effect.Effect<FileDiffPreview, never> {
	return pipe(
		file.status === "??"
			? loadUntrackedPreview(file.path)
			: loadTrackedPreview(file.path),
		Effect.map(withDefaultPreviewNote),
	);
}

export function loadBranchFilePreview(
	filePath: string,
	selection: BranchDiffSelection,
): Effect.Effect<FileDiffPreview, never> {
	const normalizedSelection = normalizeBranchDiffSelection(selection);
	return pipe(
		loadBranchPreview(filePath, normalizedSelection),
		Effect.map(withDefaultPreviewNote),
	);
}

export function loadCommitFilePreview(
	filePath: string,
	selection: CommitDiffSelection,
): Effect.Effect<FileDiffPreview, never> {
	const normalizedSelection = normalizeCommitDiffSelection(selection);
	return pipe(
		loadCommitPreview(filePath, normalizedSelection),
		Effect.map(withDefaultPreviewNote),
	);
}

export function loadFileContextLines(
	file: Pick<FileEntry, "path" | "status">,
): Effect.Effect<FileDiffContextLines, never> {
	return file.status === "??"
		? loadTextFileLines(file.path)
		: pipe(
				loadTextFileLines(file.path),
				Effect.flatMap((lines) =>
					lines.length > 0
						? Effect.succeed(lines)
						: loadRevisionFileLines("HEAD", file.path),
				),
			);
}

export function loadBranchFileContextLines(
	filePath: string,
	selection: BranchDiffSelection,
): Effect.Effect<FileDiffContextLines, never> {
	const normalizedSelection = normalizeBranchDiffSelection(selection);
	return pipe(
		loadRevisionFileLines(normalizedSelection.sourceRef, filePath),
		Effect.flatMap((lines) =>
			lines.length > 0
				? Effect.succeed(lines)
				: pipe(
						resolveBranchDiffBaseRef(normalizedSelection),
						Effect.flatMap((baseRef) =>
							baseRef.length > 0
								? loadRevisionFileLines(baseRef, filePath)
								: Effect.succeed([] as FileDiffContextLines),
						),
					),
		),
	);
}

export function loadCommitFileContextLines(
	filePath: string,
	selection: CommitDiffSelection,
): Effect.Effect<FileDiffContextLines, never> {
	const normalizedSelection = normalizeCommitDiffSelection(selection);
	return pipe(
		loadRevisionFileLines(normalizedSelection.commitHash, filePath),
		Effect.flatMap((lines) =>
			lines.length > 0
				? Effect.succeed(lines)
				: loadRevisionFileLines(normalizedSelection.baseRef, filePath),
		),
	);
}
