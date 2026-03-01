import { Effect, pipe } from "effect";
import { reviewerUsage, runReviewerCommand } from "./cmd/reviewer";

export async function runCli(argv: string[]) {
	const program = pipe(
		runReviewerCommand(argv),
		Effect.catchTag("CliArgumentError", (error) =>
			Effect.sync(() => {
				console.error(error.message);
				console.error("");
				console.error(reviewerUsage());
				process.exitCode = 1;
			}),
		),
		Effect.catchAll((error) =>
			Effect.sync(() => {
				console.error(error.message);
				process.exitCode = 1;
			}),
		),
	);

	await Effect.runPromise(program);
}
