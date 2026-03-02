import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import * as FileSystem from "@effect/platform/FileSystem";
import { Data, Effect, Option, pipe } from "effect";
import { resolveDiffFiletype } from "#syntax/tree-sitter";
import { FileEntry, type StatusEntry } from "#tui/types";

const TEXT_DECODER = new TextDecoder();

export class GitCommandError extends Data.TaggedError("GitCommandError")<{
	readonly args: ReadonlyArray<string>;
	readonly stdout: string;
	readonly stderr: string;
	readonly fallbackMessage: string;
}> {}

export class CommitMessageRequiredError extends Data.TaggedError(
	"CommitMessageRequiredError",
)<{
	readonly message: string;
}> {}

export type RepoActionError = GitCommandError | CommitMessageRequiredError;

function decodeOutput(output?: Uint8Array | null): string {
	if (!output) {
		return "";
	}
	return TEXT_DECODER.decode(output);
}

function runGitEffect(
	args: string[],
	fallbackMessage: string,
): Effect.Effect<
	{ readonly stdout: string; readonly stderr: string },
	GitCommandError
> {
	return pipe(
		Effect.sync(() =>
			Bun.spawnSync({
				cmd: ["git", ...args],
				stdout: "pipe",
				stderr: "pipe",
			}),
		),
		Effect.flatMap((result) => {
			const stdout = decodeOutput(result.stdout);
			const stderr = decodeOutput(result.stderr);
			return result.exitCode === 0
				? Effect.succeed({
						stdout,
						stderr,
					})
				: Effect.fail(
						new GitCommandError({
							args,
							stdout,
							stderr,
							fallbackMessage,
						}),
					);
		}),
	);
}

function runGitEffectAsync(
	args: string[],
	fallbackMessage: string,
): Effect.Effect<
	{ readonly stdout: string; readonly stderr: string },
	GitCommandError
> {
	return pipe(
		Effect.tryPromise({
			try: async () => {
				const process = Bun.spawn({
					cmd: ["git", ...args],
					stdout: "pipe",
					stderr: "pipe",
				});
				const [exitCode, stdout, stderr] = await Promise.all([
					process.exited,
					process.stdout
						? new Response(process.stdout).text()
						: Promise.resolve(""),
					process.stderr
						? new Response(process.stderr).text()
						: Promise.resolve(""),
				]);
				return { exitCode, stdout, stderr };
			},
			catch: () =>
				new GitCommandError({
					args,
					stdout: "",
					stderr: "",
					fallbackMessage,
				}),
		}),
		Effect.flatMap((result) =>
			result.exitCode === 0
				? Effect.succeed({
						stdout: result.stdout,
						stderr: result.stderr,
					})
				: Effect.fail(
						new GitCommandError({
							args,
							stdout: result.stdout,
							stderr: result.stderr,
							fallbackMessage,
						}),
					),
		),
	);
}

function renderGitCommandError(error: GitCommandError): string {
	return error.stderr.trim() || error.stdout.trim() || error.fallbackMessage;
}

export function isFileStaged(status: string): boolean {
	if (status === "??") {
		return false;
	}

	const indexStatus = status[0] ?? " ";
	return indexStatus !== " " && indexStatus !== "?";
}

export function toggleFileStage(
	file: Pick<FileEntry, "path" | "status">,
): Effect.Effect<void, GitCommandError> {
	const args = isFileStaged(file.status)
		? ["restore", "--staged", "--", file.path]
		: ["add", "--", file.path];
	return pipe(
		runGitEffect(args, `Unable to update staged state for ${file.path}.`),
		Effect.asVoid,
	);
}

export function discardFileChanges(
	file: Pick<FileEntry, "path" | "status">,
): Effect.Effect<void, GitCommandError> {
	const args =
		file.status === "??"
			? ["clean", "-f", "--", file.path]
			: ["restore", "--source=HEAD", "--staged", "--worktree", "--", file.path];
	return pipe(
		runGitEffect(args, `Unable to discard changes for ${file.path}.`),
		Effect.asVoid,
	);
}

export function commitStagedChanges(
	message: string,
): Effect.Effect<void, RepoActionError> {
	const trimmedMessage = message.trim();
	if (!trimmedMessage) {
		return Effect.fail(
			new CommitMessageRequiredError({
				message: "Commit message is required.",
			}),
		);
	}

	return pipe(
		runGitEffect(["commit", "-m", trimmedMessage], "Unable to create commit."),
		Effect.asVoid,
	);
}

export function pullFromRemote(): Effect.Effect<void, GitCommandError> {
	return pipe(
		runGitEffectAsync(["pull"], "Unable to pull from remote."),
		Effect.asVoid,
	);
}

export function pushToRemote(): Effect.Effect<void, GitCommandError> {
	return pipe(
		runGitEffectAsync(["push"], "Unable to push to remote."),
		Effect.asVoid,
	);
}

export function initGitRepository(): Effect.Effect<void, GitCommandError> {
	return pipe(
		runGitEffect(["init"], "Unable to initialize git repository."),
		Effect.asVoid,
	);
}

function parseStatusEntries(raw: string): StatusEntry[] {
	const entries: StatusEntry[] = [];
	const fields = raw.split("\0");
	let index = 0;

	while (index < fields.length) {
		const field = fields[index];
		index += 1;

		if (!field || field.length < 4) {
			continue;
		}

		const x = field[0] ?? " ";
		const y = field[1] ?? " ";
		const status = `${x}${y}`;
		const firstPath = field.slice(3);

		if (!firstPath) {
			continue;
		}

		if (x === "R" || x === "C") {
			const renamedTo = fields[index];
			index += 1;
			entries.push({
				status,
				path: renamedTo || firstPath,
				originalPath: firstPath,
			});
			continue;
		}

		entries.push({ status, path: firstPath });
	}

	return entries;
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

interface FilePreview {
	readonly diff: string;
	readonly note: Option.Option<string>;
}

export type FileDiffPreview = FilePreview;

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

function toFileEntry(entry: StatusEntry): FileEntry {
	const label =
		entry.originalPath === undefined
			? entry.path
			: `${entry.originalPath} -> ${entry.path}`;
	const filetype = resolveDiffFiletype(entry.path);
	return FileEntry.make({
		status: entry.status,
		path: entry.path,
		label,
		...(Option.isSome(filetype) ? { filetype: filetype.value } : {}),
	});
}

export function loadFilesWithStatus(): Effect.Effect<
	FileEntry[],
	GitCommandError
> {
	return Effect.gen(function* () {
		const statusResult = yield* runGitEffectAsync(
			["status", "--porcelain=v1", "-z", "--untracked-files=all"],
			"Unable to run git status.",
		);

		const statusEntries = parseStatusEntries(statusResult.stdout).filter(
			(entry) => entry.status !== "!!",
		);
		return statusEntries.map(toFileEntry);
	});
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
