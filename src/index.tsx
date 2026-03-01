import { startReviewerTui } from "#tui";

function parseCliOptions(argv: string[]): { chooserFilePath?: string } {
	let chooserFilePath: string | undefined;

	for (let index = 0; index < argv.length; index += 1) {
		const arg = argv[index];
		if (arg !== "--chooser-file") {
			continue;
		}

		const value = argv[index + 1];
		if (!value) {
			throw new Error("Missing value for --chooser-file");
		}
		chooserFilePath = value;
		index += 1;
	}

	return { chooserFilePath };
}

await startReviewerTui(parseCliOptions(Bun.argv.slice(2)));
