import { ReviewerCommand } from "./cmd/reviewer";

export async function runCli(argv: string[]) {
	try {
		const args = ReviewerCommand.parse(argv);
		await ReviewerCommand.handler(args);
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		console.error(message);
		console.error("");
		console.error(ReviewerCommand.usage());
		process.exitCode = 1;
	}
}
