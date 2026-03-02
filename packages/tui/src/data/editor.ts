import { spawnSync } from "node:child_process";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import * as FileSystem from "@effect/platform/FileSystem";
import { Data, Effect, Match, Option, pipe } from "effect";

function quoteShellArg(value: string): string {
	return `'${value.replace(/'/g, `'"'"'`)}'`;
}

export class EditorEnvMissingError extends Data.TaggedError(
	"EditorEnvMissingError",
)<{
	readonly message: string;
}> {}

export class EditorLaunchError extends Data.TaggedError("EditorLaunchError")<{
	readonly message: string;
}> {}

export class ChooserWriteError extends Data.TaggedError("ChooserWriteError")<{
	readonly chooserFilePath: string;
}> {}

export type OpenFileError =
	| EditorEnvMissingError
	| EditorLaunchError
	| ChooserWriteError;

function resolveEditorCommand(): Effect.Effect<string, EditorEnvMissingError> {
	return pipe(
		Option.fromNullable(process.env.VISUAL ?? process.env.EDITOR),
		Option.map((editorCommand) => editorCommand.trim()),
		Option.filter((editorCommand) => editorCommand.length > 0),
		Option.match({
			onNone: () =>
				Effect.fail(
					new EditorEnvMissingError({
						message: "Set VISUAL or EDITOR to open files from reviewer.",
					}),
				),
			onSome: (editorCommand) => Effect.succeed(editorCommand),
		}),
	);
}

export function openFileInEditor(
	filePath: string,
): Effect.Effect<void, EditorEnvMissingError | EditorLaunchError> {
	return Effect.gen(function* () {
		const editorCommand = yield* resolveEditorCommand();
		const result = yield* Effect.sync(() =>
			spawnSync("sh", ["-lc", `${editorCommand} ${quoteShellArg(filePath)}`], {
				stdio: "inherit",
			}),
		);

		if (result.error) {
			yield* Effect.fail(
				new EditorLaunchError({
					message: result.error.message || "Failed to launch editor.",
				}),
			);
		}

		if (result.status !== 0) {
			yield* Effect.fail(
				new EditorLaunchError({
					message: `Editor command exited with code ${result.status ?? 1}.`,
				}),
			);
		}
	});
}

export function writeChooserSelection(
	chooserFilePath: string,
	filePath: string,
): Effect.Effect<void, ChooserWriteError> {
	return pipe(
		Effect.gen(function* () {
			const fs = yield* FileSystem.FileSystem;
			yield* pipe(
				fs.writeFileString(chooserFilePath, `${filePath}\n`),
				Effect.catchTag(
					"SystemError",
					() => Effect.fail(new ChooserWriteError({ chooserFilePath })),
				),
				Effect.catchTag(
					"BadArgument",
					() => Effect.fail(new ChooserWriteError({ chooserFilePath })),
				),
			);
		}),
		Effect.provide(BunFileSystem.layer),
	);
}

export function renderOpenFileError(error: OpenFileError): string {
	return Match.value(error).pipe(
		Match.tag("EditorEnvMissingError", (typedError) => typedError.message),
		Match.tag("EditorLaunchError", (typedError) => typedError.message),
		Match.tag(
			"ChooserWriteError",
			(typedError) =>
				`Unable to write chooser file: ${typedError.chooserFilePath}`,
		),
		Match.exhaustive,
	);
}
