import { parseArgs } from "node:util";
import { Data, Effect, Option, pipe } from "effect";
import { type StartVigilTuiError, startVigilTuiProgram } from "#tui";

class CliArgumentError extends Data.TaggedError("CliArgumentError")<{
	readonly message: string;
}> {}

interface VigilCliArgs {
	readonly chooserFilePath: Option.Option<string>;
	readonly help: boolean;
}

function vigilUsage(): string {
	return [
		"vigil",
		"",
		"Usage:",
		"  vigil [--chooser-file <path>]",
		"",
		"Options:",
		"  --chooser-file <path>  Write selected file path and exit",
		"  -h, --help             Show help",
	].join("\n");
}

function parseVigilArgs(
	argv: string[],
): Effect.Effect<VigilCliArgs, CliArgumentError> {
	return pipe(
		Effect.try({
			try: () =>
				parseArgs({
					args: argv,
					allowPositionals: true,
					strict: true,
					options: {
						"chooser-file": {
							type: "string",
						},
						help: {
							type: "boolean",
							short: "h",
						},
					},
				}),
			catch: (error) =>
				new CliArgumentError({
					message: error instanceof Error ? error.message : String(error),
				}),
		}),
		Effect.flatMap((parsed) => {
			if (parsed.positionals.length > 0) {
				return Effect.fail(
					new CliArgumentError({
						message: `Unexpected positional arguments: ${parsed.positionals.join(" ")}`,
					}),
				);
			}

			const chooserFilePath =
				typeof parsed.values["chooser-file"] === "string"
					? Option.some(parsed.values["chooser-file"])
					: Option.none<string>();

			return Effect.succeed({
				help: parsed.values.help === true,
				chooserFilePath,
			});
		}),
	);
}

function runCli(
	argv: string[],
): Effect.Effect<void, CliArgumentError | StartVigilTuiError> {
	return Effect.gen(function* () {
		const args = yield* parseVigilArgs(argv);
		if (args.help) {
			return yield* Effect.sync(() => {
				console.log(vigilUsage());
			});
		}

		yield* startVigilTuiProgram({
			chooserFilePath: args.chooserFilePath,
		});
	});
}

await Effect.runPromise(
	pipe(
		runCli(Bun.argv.slice(2)),
		Effect.catchTag("CliArgumentError", (error) =>
			Effect.sync(() => {
				console.error(error.message);
				console.error("");
				console.error(vigilUsage());
				process.exitCode = 1;
			}),
		),
		Effect.catchAll((error) =>
			Effect.sync(() => {
				console.error(error.message);
				process.exitCode = 1;
			}),
		),
	),
);
