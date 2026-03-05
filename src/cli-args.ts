import { parseArgs } from "node:util";
import { Data, Effect, Option, pipe } from "effect";

export interface BlameTarget {
	readonly filePath: string;
	readonly lineNumber: number;
}

export class CliArgumentError extends Data.TaggedError("CliArgumentError")<{
	readonly message: string;
}> {}

type VigilCommand = "tui" | "serve";

interface ParsedCommand {
	readonly command: VigilCommand;
	readonly initialBlameTarget: Option.Option<BlameTarget>;
}

export interface VigilCliArgs {
	readonly command: VigilCommand;
	readonly chooserFilePath: Option.Option<string>;
	readonly initialBlameTarget: Option.Option<BlameTarget>;
	readonly serverHost: string;
	readonly serverPort: number;
	readonly help: boolean;
}

export function vigilUsage(): string {
	return [
		"vigil",
		"",
		"Usage:",
		"  vigil [--chooser-file <path>]",
		"  vigil blame <file>:<line>",
		"  vigil serve [--host <hostname>] [--port <number>]",
		"",
		"Options:",
		"  --chooser-file <path>  Write selected file path and exit",
		"  --host <hostname>      Hostname for `serve` mode (default: 127.0.0.1)",
		"  --port <number>        Port for `serve` mode (default: 4096)",
		"  -h, --help             Show help",
	].join("\n");
}

export function parseBlameTarget(
	rawTarget: string,
): Effect.Effect<BlameTarget, CliArgumentError> {
	const separatorIndex = rawTarget.lastIndexOf(":");
	if (separatorIndex <= 0 || separatorIndex >= rawTarget.length - 1) {
		return Effect.fail(
			new CliArgumentError({
				message: `Invalid blame target: ${rawTarget}. Expected <file>:<line>.`,
			}),
		);
	}

	const filePath = rawTarget.slice(0, separatorIndex).trim();
	const rawLineNumber = rawTarget.slice(separatorIndex + 1).trim();
	if (filePath.length === 0) {
		return Effect.fail(
			new CliArgumentError({
				message: `Invalid blame target: ${rawTarget}. File path is required.`,
			}),
		);
	}

	const lineNumber = Number(rawLineNumber);
	if (!Number.isInteger(lineNumber) || lineNumber < 1) {
		return Effect.fail(
			new CliArgumentError({
				message: `Invalid blame line number: ${rawLineNumber}.`,
			}),
		);
	}

	return Effect.succeed({
		filePath,
		lineNumber,
	});
}

function parseCommand(
	positionals: ReadonlyArray<string>,
): Effect.Effect<ParsedCommand, CliArgumentError> {
	const [first, second, ...rest] = positionals;

	if (first === undefined) {
		return Effect.succeed({
			command: "tui",
			initialBlameTarget: Option.none(),
		});
	}

	if (first === "serve") {
		if (second !== undefined || rest.length > 0) {
			return Effect.fail(
				new CliArgumentError({
					message: `Unexpected positional arguments: ${positionals.join(" ")}`,
				}),
			);
		}

		return Effect.succeed({
			command: "serve",
			initialBlameTarget: Option.none(),
		});
	}

	if (first === "blame") {
		if (second === undefined) {
			return Effect.fail(
				new CliArgumentError({
					message: "Missing blame target. Expected `vigil blame <file>:<line>`.",
				}),
			);
		}
		if (rest.length > 0) {
			return Effect.fail(
				new CliArgumentError({
					message: `Unexpected positional arguments: ${rest.join(" ")}`,
				}),
			);
		}

		return pipe(
			parseBlameTarget(second),
			Effect.map((target) => ({
				command: "tui" as const,
				initialBlameTarget: Option.some(target),
			})),
		);
	}

	return Effect.fail(
		new CliArgumentError({
			message: `Unknown command: ${first}`,
		}),
	);
}

function parseServerPort(
	rawPort: string,
): Effect.Effect<number, CliArgumentError> {
	const port = Number(rawPort);

	return Number.isInteger(port) && port >= 0 && port <= 65_535
		? Effect.succeed(port)
		: Effect.fail(
				new CliArgumentError({
					message: `Invalid --port value: ${rawPort}`,
				}),
			);
}

export function parseVigilArgs(
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
				const parsedCommand = yield* parseCommand(parsed.positionals);
				const chooserFilePath = pipe(
					Option.fromNullable(parsed.values["chooser-file"]),
					Option.filter((value): value is string => typeof value === "string"),
				);

				if (parsedCommand.command === "serve" && Option.isSome(chooserFilePath)) {
					return yield* new CliArgumentError({
						message: "`--chooser-file` can only be used in TUI mode.",
					});
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
					command: parsedCommand.command,
					chooserFilePath,
					initialBlameTarget: parsedCommand.initialBlameTarget,
					help: parsed.values.help === true,
					serverHost,
					serverPort,
				} satisfies VigilCliArgs;
			}),
		),
	);
}
