import { spawn } from "node:child_process";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import { parseArgs } from "node:util";
import {
	loadOrCreateDaemonTokenFromTuiConfig,
} from "@vigil/config";
import {
	DaemonMetaResponse,
	HealthResponse,
	type VigilServerStartError,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	VIGIL_DAEMON_TOKEN_HEADER,
	startVigilServerProgram,
} from "@vigil/server";
import { type StartVigilTuiError, startVigilTuiProgram } from "@vigil/tui";
import { Data, Effect, Option, Schema, pipe } from "effect";

const DAEMON_POLL_INTERVAL_MS = 100;
const DAEMON_POLL_ATTEMPTS = 50;

const decodeDaemonMetaResponse = Schema.decodeUnknownSync(DaemonMetaResponse);
const decodeHealthResponse = Schema.decodeUnknownSync(HealthResponse);

class CliArgumentError extends Data.TaggedError("CliArgumentError")<{
	readonly message: string;
}> {}

class VigilDaemonEnsureError extends Data.TaggedError("VigilDaemonEnsureError")<{
	readonly message: string;
	readonly cause?: unknown;
}> {}

type VigilCommand = "tui" | "serve";

interface VigilCliArgs {
	readonly command: VigilCommand;
	readonly chooserFilePath: Option.Option<string>;
	readonly serverHost: string;
	readonly serverPort: number;
	readonly help: boolean;
}

interface DaemonConnectionOptions {
	readonly host: string;
	readonly port: number;
	readonly daemonToken: string;
}

type DaemonProbeResult =
	| {
			readonly _tag: "ready";
	  }
	| {
			readonly _tag: "unauthorized";
	  }
	| {
			readonly _tag: "unreachable";
			readonly detail: string;
	  };

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

function resolveDaemonToken(): Effect.Effect<string, VigilDaemonEnsureError> {
	const tokenFromEnv = process.env[VIGIL_DAEMON_TOKEN_ENV_VAR]?.trim();
	return tokenFromEnv && tokenFromEnv.length > 0
		? Effect.succeed(tokenFromEnv)
		: pipe(
				loadOrCreateDaemonTokenFromTuiConfig(),
				Effect.provide(BunFileSystem.layer),
				Effect.mapError(
					(cause) =>
						new VigilDaemonEnsureError({
							message: cause.message,
							cause,
						}),
				),
			);
}

function daemonBaseUrl(options: Pick<DaemonConnectionOptions, "host" | "port">) {
	const host = options.host.includes(":")
		? `[${options.host.replace(/^\[(.*)\]$/, "$1")}]`
		: options.host;
	return `http://${host}:${options.port}`;
}

function probeDaemon(
	options: DaemonConnectionOptions,
): Effect.Effect<DaemonProbeResult, never> {
	return pipe(
		Effect.tryPromise({
			try: async () => {
				const baseUrl = daemonBaseUrl(options);
				const headers = {
					[VIGIL_DAEMON_TOKEN_HEADER]: options.daemonToken,
				} satisfies Record<string, string>;

				const metaResponse = await fetch(`${baseUrl}/meta`, { headers });
				if (metaResponse.status === 401) {
					return { _tag: "unauthorized" } as const;
				}
				if (!metaResponse.ok) {
					return {
						_tag: "unreachable",
						detail: `GET /meta returned status ${metaResponse.status}.`,
					} as const;
				}

				decodeDaemonMetaResponse(await metaResponse.json());

				const healthResponse = await fetch(`${baseUrl}/health`, { headers });
				if (healthResponse.status === 401) {
					return { _tag: "unauthorized" } as const;
				}
				if (!healthResponse.ok) {
					return {
						_tag: "unreachable",
						detail: `GET /health returned status ${healthResponse.status}.`,
					} as const;
				}

				decodeHealthResponse(await healthResponse.json());
				return { _tag: "ready" } as const;
			},
			catch: (cause) => cause,
		}),
		Effect.catchAll((cause) =>
			Effect.succeed({
				_tag: "unreachable" as const,
				detail: cause instanceof Error ? cause.message : String(cause),
			}),
		),
	);
}

function spawnDaemonProcess(
	options: DaemonConnectionOptions,
): Effect.Effect<void, VigilDaemonEnsureError> {
	return Effect.try({
		try: () => {
			const entrypoint = process.argv[1];
			if (!entrypoint) {
				throw new Error("Unable to resolve CLI entrypoint.");
			}

			const processHandle = spawn(
				process.execPath,
				[
					entrypoint,
					"serve",
					"--host",
					options.host,
					"--port",
					String(options.port),
				],
				{
					detached: true,
					stdio: "ignore",
					windowsHide: true,
					env: {
						...process.env,
						[VIGIL_DAEMON_TOKEN_ENV_VAR]: options.daemonToken,
					},
				},
			);

			processHandle.unref();
		},
		catch: (cause) =>
			new VigilDaemonEnsureError({
				message: "Failed to spawn daemon process.",
				cause,
			}),
	});
}

function ensureServer(
	options: Pick<DaemonConnectionOptions, "host" | "port" | "daemonToken">,
): Effect.Effect<void, VigilDaemonEnsureError> {
	return Effect.gen(function* () {
		const connectionOptions: DaemonConnectionOptions = {
			host: options.host,
			port: options.port,
			daemonToken: options.daemonToken,
		};

		const firstProbe = yield* probeDaemon(connectionOptions);
		if (firstProbe._tag === "ready") {
			return;
		}

		if (firstProbe._tag === "unauthorized") {
			return yield* new VigilDaemonEnsureError({
				message: `A server is already listening at ${daemonBaseUrl(connectionOptions)} but rejected this daemon token.`,
			});
		}

		yield* spawnDaemonProcess(connectionOptions);

		let lastProbeDetail = firstProbe.detail;
		for (let attempt = 0; attempt < DAEMON_POLL_ATTEMPTS; attempt += 1) {
			const probe = yield* probeDaemon(connectionOptions);
			if (probe._tag === "ready") {
				return;
			}
			if (probe._tag === "unauthorized") {
				return yield* new VigilDaemonEnsureError({
					message: `Daemon at ${daemonBaseUrl(connectionOptions)} rejected this daemon token after startup.`,
				});
			}

			lastProbeDetail = probe.detail;
			yield* Effect.sleep(DAEMON_POLL_INTERVAL_MS);
		}

		return yield* new VigilDaemonEnsureError({
			message: `Unable to connect to daemon at ${daemonBaseUrl(connectionOptions)} after auto-start. Last check: ${lastProbeDetail}`,
		});
	});
}

function runCli(
	argv: string[],
): Effect.Effect<
	void,
	| CliArgumentError
	| StartVigilTuiError
	| VigilServerStartError
	| VigilDaemonEnsureError
> {
	return Effect.gen(function* () {
		const args = yield* parseVigilArgs(argv);
		if (args.help) {
			return yield* Effect.sync(() => {
				console.log(vigilUsage());
			});
		}

		const daemonToken = yield* resolveDaemonToken();

		if (args.command === "serve") {
			return yield* startVigilServerProgram({
				host: args.serverHost,
				port: args.serverPort,
				daemonToken,
			});
		}

		yield* ensureServer({
			host: args.serverHost,
			port: args.serverPort,
			daemonToken,
		});

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
		Effect.catchTag("VigilDaemonEnsureError", (error) =>
			printErrorAndExit(error.message),
		),
		Effect.catchAll((error) => printErrorAndExit(error.message)),
	),
);
