import { Data, Effect, pipe } from "effect";

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

export interface BranchDiffSelection {
	readonly sourceRef: string;
	readonly destinationRef: string;
}

function decodeOutput(output?: Uint8Array | null): string {
	if (!output) {
		return "";
	}
	return TEXT_DECODER.decode(output);
}

export function runGitEffect(
	args: ReadonlyArray<string>,
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
							args: [...args],
							stdout,
							stderr,
							fallbackMessage,
						}),
					);
		}),
	);
}

export function runGitEffectAsync(
	args: ReadonlyArray<string>,
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
					args: [...args],
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
							args: [...args],
							stdout: result.stdout,
							stderr: result.stderr,
							fallbackMessage,
						}),
					),
		),
	);
}

export function renderGitCommandError(error: GitCommandError): string {
	return error.stderr.trim() || error.stdout.trim() || error.fallbackMessage;
}

export function buildBranchDiffRange(selection: BranchDiffSelection): string {
	return `${selection.destinationRef}...${selection.sourceRef}`;
}

export function normalizeBranchDiffSelection(
	selection: BranchDiffSelection,
): BranchDiffSelection {
	return {
		sourceRef: selection.sourceRef.trim(),
		destinationRef: selection.destinationRef.trim(),
	};
}
