import { parseArgs } from "node:util";
import { startReviewerTui } from "#tui";
import { cmd } from "./cmd";

interface ReviewerCliArgs {
	chooserFilePath?: string;
	help: boolean;
}

function parseReviewerArgs(argv: string[]): ReviewerCliArgs {
	const parsed = parseArgs({
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

	if (parsed.positionals.length > 0) {
		throw new Error(
			`Unexpected positional arguments: ${parsed.positionals.join(" ")}`,
		);
	}

	return {
		chooserFilePath:
			typeof parsed.values["chooser-file"] === "string"
				? parsed.values["chooser-file"]
				: undefined,
		help: parsed.values.help === true,
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

		await startReviewerTui({ chooserFilePath: args.chooserFilePath });
	},
});
