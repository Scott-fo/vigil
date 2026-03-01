import { parseArgs } from "node:util";
import { Data, Effect, Option, pipe } from "effect";
import {
	type StartReviewerTuiError,
	startReviewerTuiProgram,
} from "#tui/bootstrap";

export class CliArgumentError extends Data.TaggedError("CliArgumentError")<{
	readonly message: string;
}> {}

interface ReviewerCliArgs {
	readonly chooserFilePath: Option.Option<string>;
	readonly help: boolean;
}

function parseReviewerArgs(
	argv: string[],
): Effect.Effect<ReviewerCliArgs, CliArgumentError> {
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

export function reviewerUsage(): string {
	return [
		"reviewer",
		"",
		"Usage:",
		"  reviewer [--chooser-file <path>]",
		"",
		"Options:",
		"  --chooser-file <path>  Write selected file path and exit",
		"  -h, --help             Show help",
	].join("\n");
}

export function runReviewerCommand(
	argv: string[],
): Effect.Effect<void, CliArgumentError | StartReviewerTuiError> {
	return Effect.gen(function* () {
		const args = yield* parseReviewerArgs(argv);
		if (args.help) {
			return yield* Effect.sync(() => {
				console.log(reviewerUsage());
			});
		}

		yield* startReviewerTuiProgram(
			pipe(
				args.chooserFilePath,
				Option.match({
					onNone: () => ({}),
					onSome: (chooserFilePath) => ({ chooserFilePath }),
				}),
			),
		);
	});
}
