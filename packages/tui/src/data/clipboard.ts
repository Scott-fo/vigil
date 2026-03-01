import os from "node:os";
import { Data, Effect, Option, pipe } from "effect";

interface ClipboardRenderer {
	copyToClipboardOSC52(text: string): boolean;
}

export class NativeClipboardCopyError extends Data.TaggedError(
	"NativeClipboardCopyError",
)<{
	readonly command: ReadonlyArray<string>;
	readonly message: string;
	readonly cause: unknown;
}> {}

export class ClipboardUnavailableError extends Data.TaggedError(
	"ClipboardUnavailableError",
)<{
	readonly message: string;
}> {}

export type ClipboardCopyError =
	| NativeClipboardCopyError
	| ClipboardUnavailableError;

function resolveClipboardCommand(): Option.Option<ReadonlyArray<string>> {
	const platform = os.platform();
	if (platform === "darwin" && Bun.which("pbcopy")) {
		return Option.some(["pbcopy"]);
	}

	if (platform === "linux") {
		if (process.env.WAYLAND_DISPLAY && Bun.which("wl-copy")) {
			return Option.some(["wl-copy"]);
		}
		if (Bun.which("xclip")) {
			return Option.some(["xclip", "-selection", "clipboard"]);
		}
		if (Bun.which("xsel")) {
			return Option.some(["xsel", "--clipboard", "--input"]);
		}
	}

	if (platform === "win32" && Bun.which("clip")) {
		return Option.some(["clip"]);
	}

	return Option.none();
}

function runNativeClipboardCommand(
	command: ReadonlyArray<string>,
	text: string,
): Effect.Effect<void, NativeClipboardCopyError> {
	return pipe(
		Effect.tryPromise({
			try: async () => {
				const process = Bun.spawn([...command], {
					stdin: "pipe",
					stdout: "ignore",
					stderr: "pipe",
				});
				if (!process.stdin) {
					throw new Error("Native clipboard command has no stdin pipe.");
				}
				process.stdin.write(text);
				process.stdin.end();
				const [exitCode, stderr] = await Promise.all([
					process.exited,
					new Response(process.stderr).text(),
				]);
				if (exitCode === 0) {
					return;
				}
				throw new Error(stderr.trim() || "Native clipboard command failed.");
			},
			catch: (cause) =>
				new NativeClipboardCopyError({
					command,
					message: `Clipboard command failed: ${command.join(" ")}`,
					cause,
				}),
		}),
	);
}

export function copyTextToClipboard(
	renderer: ClipboardRenderer,
	text: string,
): Effect.Effect<void, ClipboardCopyError> {
	return Effect.gen(function* () {
		const osc52Copied = renderer.copyToClipboardOSC52(text);
		const commandOption = resolveClipboardCommand();
		if (Option.isSome(commandOption)) {
			const nativeResult = yield* pipe(
				runNativeClipboardCommand(commandOption.value, text),
				Effect.as(Option.none<NativeClipboardCopyError>()),
				Effect.catchTag("NativeClipboardCopyError", (error) =>
					Effect.succeed(Option.some(error)),
				),
			);
			if (Option.isNone(nativeResult)) {
				return;
			}

			if (osc52Copied) {
				return;
			}

			return yield* Effect.fail(
				new NativeClipboardCopyError({
					command: nativeResult.value.command,
					message: nativeResult.value.message,
					cause: nativeResult.value.cause,
				}),
			);
		}

		if (osc52Copied) {
			return;
		}

		return yield* Effect.fail(
			new ClipboardUnavailableError({
				message: "Unable to copy text to clipboard in this terminal/session.",
			}),
		);
	});
}
