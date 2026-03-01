import { Data, Match, Predicate, pipe } from "effect";
import { ReviewerCommand } from "./cmd/reviewer";
import { CliArgumentError } from "./cmd/reviewer";

class UnknownCliError extends Data.TaggedError("UnknownCliError")<{
	readonly message: string;
}> {}

type CliRunError = CliArgumentError | UnknownCliError;

function normalizeCliError(error: unknown): CliRunError {
	if (Predicate.isTagged("CliArgumentError")(error)) {
		return error as CliArgumentError;
	}

	if (error instanceof Error) {
		return new UnknownCliError({ message: error.message });
	}

	return new UnknownCliError({ message: String(error) });
}

export async function runCli(argv: string[]) {
	try {
		const args = ReviewerCommand.parse(argv);
		await ReviewerCommand.handler(args);
	} catch (error) {
		const message = pipe(
			normalizeCliError(error),
			Match.value,
			Match.tag("CliArgumentError", (typedError) => typedError.message),
			Match.tag("UnknownCliError", (typedError) => typedError.message),
			Match.exhaustive,
		);
		console.error(message);
		console.error("");
		console.error(ReviewerCommand.usage());
		process.exitCode = 1;
	}
}
