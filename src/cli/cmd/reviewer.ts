import { parseArgs } from "node:util";
import { Data, Option, pipe } from "effect";
import { startReviewerTui } from "#tui";
import { cmd } from "./cmd";

export class CliArgumentError extends Data.TaggedError("CliArgumentError")<{
	readonly message: string;
}> {}

interface ReviewerCliArgs {
	chooserFilePath: Option.Option<string>;
	help: boolean;
}

function parseReviewerArgs(argv: string[]): ReviewerCliArgs {
	let parsed: ReturnType<typeof parseArgs>;
	try {
		parsed = parseArgs({
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
		});
	} catch (error) {
		throw new CliArgumentError({
			message: error instanceof Error ? error.message : String(error),
		});
	}

	if (parsed.positionals.length > 0) {
		throw new CliArgumentError({
			message: `Unexpected positional arguments: ${parsed.positionals.join(" ")}`,
		});
	}

	const chooserFilePath =
		typeof parsed.values["chooser-file"] === "string"
			? Option.some(parsed.values["chooser-file"])
			: Option.none<string>();

	return {
		help: parsed.values.help === true,
		chooserFilePath,
	};
}

function reviewerUsage(): string {
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

export const ReviewerCommand = cmd<ReviewerCliArgs>({
	command: "$0",
	describe: "start reviewer tui",
	usage: reviewerUsage,
	parse: parseReviewerArgs,
	handler: async (args) => {
		if (args.help) {
			console.log(reviewerUsage());
			return;
		}

		await startReviewerTui(
			pipe(
				args.chooserFilePath,
				Option.match({
					onNone: () => undefined,
					onSome: (chooserFilePath) => ({ chooserFilePath }),
				}),
			),
		);
	},
});
