import { Data, Effect, Option, pipe } from "effect";
import { resolveDiffFiletype } from "#syntax/tree-sitter";
import type { FileEntry, GitCommandResult, StatusEntry } from "#tui/types";

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

function runGit(args: string[]): GitCommandResult {
	const result = Bun.spawnSync({
		cmd: ["git", ...args],
		stdout: "pipe",
		stderr: "pipe",
	});

	return {
		ok: result.exitCode === 0,
		stdout: decodeOutput(result.stdout),
		stderr: decodeOutput(result.stderr),
	};
}

function runGitEffect(
	args: string[],
	fallbackMessage: string,
): Effect.Effect<GitCommandResult, GitCommandError> {
	return pipe(
		Effect.sync(() => runGit(args)),
		Effect.flatMap((result) =>
			result.ok
				? Effect.succeed(result)
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
	return (
		error.stderr.trim() || error.stdout.trim() || error.fallbackMessage
	);
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
		runGitEffect(["pull"], "Unable to pull from remote."),
		Effect.asVoid,
	);
}

export function pushToRemote(): Effect.Effect<void, GitCommandError> {
	return pipe(
		runGitEffect(["push"], "Unable to push to remote."),
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

function inferFiletype(inputPath: string): Option.Option<string> {
	return resolveDiffFiletype(inputPath);
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

export function loadFilesWithDiffs(): Effect.Effect<FileEntry[], GitCommandError> {
	return Effect.gen(function* () {
		const statusResult = yield* runGitEffect(
			["status", "--porcelain=v1", "-z", "--untracked-files=all"],
			"Unable to run git status.",
		);

		const statusEntries = parseStatusEntries(statusResult.stdout).filter(
			(entry) => entry.status !== "!!",
		);
		const files: FileEntry[] = [];

		for (const entry of statusEntries) {
			const label = entry.originalPath
				? `${entry.originalPath} -> ${entry.path}`
				: entry.path;
			let diff = "";
			let note = Option.none<string>();

			if (entry.status === "??") {
				const untrackedRead = yield* pipe(
					Effect.tryPromise(() => Bun.file(entry.path).bytes()),
					Effect.match({
						onFailure: () => ({ ok: false as const }),
						onSuccess: (bytes) => ({ ok: true as const, bytes }),
					}),
				);

				if (!untrackedRead.ok) {
					note = Option.some("Unable to read untracked file content.");
				} else {
					const hasNullByte = untrackedRead.bytes.includes(0);
					if (hasNullByte) {
						note = Option.some("Binary or non-text file; no preview available.");
					} else {
						const content = TEXT_DECODER.decode(untrackedRead.bytes);
						diff = createUntrackedFileDiff(entry.path, content);
						if (!diff.trim()) {
							note = Option.some(
								"Untracked empty file; no textual hunk to preview.",
							);
						}
					}
				}
			} else {
				const trackedDiff = yield* pipe(
					runGitEffect(
						["diff", "--no-color", "--find-renames", "HEAD", "--", entry.path],
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
				diff = trackedDiff.diff;
				note = trackedDiff.note;
			}

			if (!diff.trim() && Option.isNone(note)) {
				note = Option.some("No textual diff available.");
			}

			const filetype = inferFiletype(entry.path);
			const fileEntryBase: FileEntry = {
				status: entry.status,
				path: entry.path,
				label,
				diff,
			};
			const fileEntryWithFiletype = pipe(
				filetype,
				Option.match({
					onNone: () => fileEntryBase,
					onSome: (resolvedFiletype) => ({
						...fileEntryBase,
						filetype: resolvedFiletype,
					}),
				}),
			);
			const fileEntry = pipe(
				note,
				Option.match({
					onNone: () => fileEntryWithFiletype,
					onSome: (resolvedNote) => ({
						...fileEntryWithFiletype,
						note: resolvedNote,
					}),
				}),
			);
			files.push(fileEntry);
		}

		return files;
	});
}
