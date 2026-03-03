import { parseArgs } from "node:util";
import { Data, Effect, Option, pipe } from "effect";
import {
	type VigilServerStartError,
	startVigilServerProgram,
} from "#server";
import { type StartVigilTuiError, startVigilTuiProgram } from "#tui";

class CliArgumentError extends Data.TaggedError("CliArgumentError")<{
	readonly message: string;
}> {}

type VigilCommand = "tui" | "serve";

interface VigilCliArgs {
	readonly command: VigilCommand;
	readonly chooserFilePath: Option.Option<string>;
	readonly serverHost: string;
	readonly serverPort: number;
	readonly help: boolean;
}

function vigilUsage(): string {
	return [
		"vigil",
		"",
		"Usage:",
		"  vigil [--chooser-file <path>]",
		"  vigil serve [--host <hostname>] [--port <number>]",
		"",
		"Options:",
		"  --chooser-file <path>  Write selected file path and exit",
		"  --host <hostname>      Hostname for `serve` mode (default: 127.0.0.1)",
		"  --port <number>        Port for `serve` mode (default: 4096)",
		"  -h, --help             Show help",
	].join("\n");
}

function parseCommand(
	positionals: ReadonlyArray<string>,
): Effect.Effect<VigilCommand, CliArgumentError> {
	if (positionals.length > 1) {
		return Effect.fail(
			new CliArgumentError({
				message: `Unexpected positional arguments: ${positionals.join(" ")}`,
			}),
		);
	}

	return pipe(
		Option.fromNullable(positionals[0]),
		Option.match({
			onNone: () => Effect.succeed("tui" as const),
			onSome: (command) =>
				command === "serve"
					? Effect.succeed("serve" as const)
					: Effect.fail(
							new CliArgumentError({
								message: `Unknown command: ${command}`,
							}),
						),
		}),
	);
}

function parseServerPort(rawPort: string): Effect.Effect<number, CliArgumentError> {
	const port = Number(rawPort);

	return Number.isInteger(port) && port >= 0 && port <= 65_535
		? Effect.succeed(port)
		: Effect.fail(
				new CliArgumentError({
					message: `Invalid --port value: ${rawPort}`,
				}),
			);
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
						host: {
							type: "string",
						},
						port: {
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
			Effect.flatMap((parsed) =>
				Effect.gen(function* () {
					const command = yield* parseCommand(parsed.positionals);
					const chooserFilePath = pipe(
						Option.fromNullable(parsed.values["chooser-file"]),
						Option.filter((value): value is string => typeof value === "string"),
					);

					if (command === "serve" && Option.isSome(chooserFilePath)) {
						return yield* Effect.fail(
							new CliArgumentError({
								message: "`--chooser-file` can only be used in TUI mode.",
							}),
						);
					}

					const serverHost = pipe(
						Option.fromNullable(parsed.values.host),
						Option.filter((value): value is string => typeof value === "string"),
						Option.getOrElse(() => "127.0.0.1"),
					);
					const rawPort = pipe(
						Option.fromNullable(parsed.values.port),
						Option.filter((value): value is string => typeof value === "string"),
						Option.getOrElse(() => "4096"),
					);
					const serverPort = yield* parseServerPort(rawPort);

					return {
						command,
						help: parsed.values.help === true,
						chooserFilePath,
						serverHost,
						serverPort,
					};
				}),
			),
	);
}

function runCli(
	argv: string[],
): Effect.Effect<
	void,
	CliArgumentError | StartVigilTuiError | VigilServerStartError
> {
	return Effect.gen(function* () {
		const args = yield* parseVigilArgs(argv);
		if (args.help) {
			return yield* Effect.sync(() => {
				console.log(vigilUsage());
			});
		}

		if (args.command === "serve") {
			return yield* startVigilServerProgram({
				host: args.serverHost,
				port: args.serverPort,
			});
		}

		yield* startVigilTuiProgram({
			chooserFilePath: args.chooserFilePath,
		});
	});
}

function printErrorAndExit(message: string): Effect.Effect<void> {
	return Effect.sync(() => {
		console.error(message);
		process.exitCode = 1;
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
		Effect.catchTag("VigilServerStartError", (error) =>
			printErrorAndExit(error.message),
		),
		Effect.catchAll((error) => printErrorAndExit(error.message)),
	),
);
