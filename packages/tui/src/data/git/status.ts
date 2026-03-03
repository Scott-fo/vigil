import { Effect, pipe } from "effect";
import {
	CommitMessageRequiredError,
	type GitCommandError,
	type RepoActionError,
	runGitEffect,
	runGitEffectAsync,
} from "#data/git/core.ts";
import { parseStatusEntries, toFileEntry } from "#data/git/parsers.ts";
import type { FileEntry } from "#tui/types.ts";

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
