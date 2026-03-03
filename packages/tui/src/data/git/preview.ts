import * as FileSystem from "@effect/platform/FileSystem";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import { Effect, Option, pipe } from "effect";
import {
	type BranchDiffSelection,
	buildBranchDiffRange,
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
