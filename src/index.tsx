import { spawn } from "node:child_process";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import { loadOrCreateDaemonTokenFromTuiConfig } from "@vigil/config";
import { parseArgs } from "node:util";
import {
	buildVigilDaemonBaseUrl,
	DaemonUnauthorizedError,
	makeVigilDaemonClient,
	makeVigilDaemonHttpClientLayer,
	type VigilServerStartError,
	startVigilServerProgram,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	type VigilDaemonConnection,
} from "@vigil/server";
import { type StartVigilTuiError, startVigilTuiProgram } from "@vigil/tui";
import { Data, Effect, Either, Option, pipe } from "effect";

const DAEMON_POLL_INTERVAL_MS = 100;
const DAEMON_POLL_ATTEMPTS = 50;
const VIGIL_DAEMON_MANAGED_ENV_VAR = "VIGIL_DAEMON_MANAGED";
const MIN_DAEMON_HEARTBEAT_MS = 500;

class CliArgumentError extends Data.TaggedError("CliArgumentError")<{
	readonly message: string;
}> {}

class VigilDaemonEnsureError extends Data.TaggedError("VigilDaemonEnsureError")<{
	readonly message: string;
	readonly cause?: unknown;
}> {}

class DaemonProbeUnreachableError extends Data.TaggedError(
	"DaemonProbeUnreachableError",
)<{
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

interface DaemonSessionLease {
	readonly sessionId: string;
	readonly ttlMs: number;
	readonly heartbeatIntervalMs: number;
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
	return buildVigilDaemonBaseUrl(options);
}

function toVigilDaemonConnection(
	options: DaemonConnectionOptions,
): VigilDaemonConnection {
	return {
		host: options.host,
		port: options.port,
		token: options.daemonToken,
	};
}

function makeDaemonProbeClient(options: DaemonConnectionOptions) {
	const daemonConnection = toVigilDaemonConnection(options);

	return makeVigilDaemonClient(daemonConnection).pipe(
		Effect.provide(makeVigilDaemonHttpClientLayer(daemonConnection)),
	);
}

function probeDaemon(
	options: DaemonConnectionOptions,
): Effect.Effect<void, DaemonUnauthorizedError | DaemonProbeUnreachableError> {
	return Effect.gen(function* () {
		const daemonClient = yield* makeDaemonProbeClient(options);
		const metaResult = yield* daemonClient.system.meta().pipe(Effect.either);
		if (Either.isLeft(metaResult)) {
			const error = metaResult.left;
			if (error._tag === "DaemonUnauthorizedError") {
				return yield* Effect.fail(error);
			}
			return yield* new DaemonProbeUnreachableError({
				message: error.message,
				cause: error,
			});
		}

		const healthResult = yield* daemonClient.system.health().pipe(Effect.either);
		if (Either.isLeft(healthResult)) {
			const error = healthResult.left;
			if (error._tag === "DaemonUnauthorizedError") {
				return yield* Effect.fail(error);
			}
			return yield* new DaemonProbeUnreachableError({
				message: error.message,
				cause: error,
			});
		}
	});
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
						[VIGIL_DAEMON_MANAGED_ENV_VAR]: "1",
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

		const firstProbeDetail = yield* probeDaemon(connectionOptions).pipe(
			Effect.as(Option.none<string>()),
			Effect.catchTag("DaemonUnauthorizedError", () =>
				Effect.fail(
					new VigilDaemonEnsureError({
						message: `A server is already listening at ${daemonBaseUrl(connectionOptions)} but rejected this daemon token.`,
					}),
				),
			),
			Effect.catchTag("DaemonProbeUnreachableError", (error) =>
				Effect.succeed(Option.some(error.message)),
			),
		);
		if (Option.isNone(firstProbeDetail)) {
			return;
		}

		yield* spawnDaemonProcess(connectionOptions);

		let lastProbeDetail = firstProbeDetail.value;
		for (let attempt = 0; attempt < DAEMON_POLL_ATTEMPTS; attempt += 1) {
			const probeDetail = yield* probeDaemon(connectionOptions).pipe(
				Effect.as(Option.none<string>()),
				Effect.catchTag("DaemonUnauthorizedError", () =>
					Effect.fail(
						new VigilDaemonEnsureError({
							message: `Daemon at ${daemonBaseUrl(connectionOptions)} rejected this daemon token after startup.`,
						}),
					),
				),
				Effect.catchTag("DaemonProbeUnreachableError", (error) =>
					Effect.succeed(Option.some(error.message)),
				),
			);
			if (Option.isNone(probeDetail)) {
				return;
			}

			lastProbeDetail = probeDetail.value;
			yield* Effect.sleep(DAEMON_POLL_INTERVAL_MS);
		}

		return yield* new VigilDaemonEnsureError({
			message: `Unable to connect to daemon at ${daemonBaseUrl(connectionOptions)} after auto-start. Last check: ${lastProbeDetail}`,
		});
	});
}

const openDaemonSession = Effect.fn("vigil.openDaemonSession")(function* (
	options: DaemonConnectionOptions,
) {
	const daemonClient = yield* makeDaemonProbeClient(options);
	const lease = yield* daemonClient.session.open().pipe(
		Effect.mapError(
			(cause) =>
				new VigilDaemonEnsureError({
					message: `Failed to open daemon session at ${daemonBaseUrl(options)}.`,
					cause,
				}),
		),
	);

	return {
		sessionId: lease.sessionId,
		ttlMs: lease.ttlMs,
		heartbeatIntervalMs: lease.heartbeatIntervalMs,
	} satisfies DaemonSessionLease;
});

const heartbeatDaemonSession = Effect.fn("vigil.heartbeatDaemonSession")(function* (
	options: DaemonConnectionOptions,
	sessionId: string,
) {
	const daemonClient = yield* makeDaemonProbeClient(options);

	yield* daemonClient.session
		.heartbeat({
			payload: {
				sessionId,
			},
		})
		.pipe(
			Effect.mapError(
				(cause) =>
					new VigilDaemonEnsureError({
						message: `Failed to heartbeat daemon session ${sessionId}.`,
						cause,
					}),
			),
		);
});

const closeDaemonSession = Effect.fn("vigil.closeDaemonSession")(function* (
	options: DaemonConnectionOptions,
	sessionId: string,
) {
	const daemonClient = yield* makeDaemonProbeClient(options);

	yield* daemonClient.session
		.close({
			payload: {
				sessionId,
			},
		})
		.pipe(
			Effect.mapError(
				(cause) =>
					new VigilDaemonEnsureError({
						message: `Failed to close daemon session ${sessionId}.`,
						cause,
					}),
			),
		);
});

const runDaemonHeartbeatLoop = Effect.fn("vigil.runDaemonHeartbeatLoop")(function* (
	options: DaemonConnectionOptions,
	lease: DaemonSessionLease,
) {
	const heartbeatIntervalMs = Math.max(
		MIN_DAEMON_HEARTBEAT_MS,
		lease.heartbeatIntervalMs,
	);

	yield* Effect.forever(
		Effect.sleep(`${heartbeatIntervalMs} millis`).pipe(
			Effect.zipRight(
				heartbeatDaemonSession(options, lease.sessionId).pipe(
					Effect.catchTag("VigilDaemonEnsureError", (error) =>
						Effect.logWarning(
							`[vigil] daemon heartbeat failed for session ${lease.sessionId}: ${error.message}`,
						),
					),
				),
			),
		),
	);
});

function withOptionalDaemonSession<A, E, R>(
	options: DaemonConnectionOptions,
	effect: Effect.Effect<A, E, R>,
): Effect.Effect<A, E | VigilDaemonEnsureError, R> {
	return Effect.gen(function* () {
		const maybeLease = yield* openDaemonSession(options).pipe(
			Effect.tap((lease) =>
				Effect.logInfo(
					`[vigil] opened daemon session ${lease.sessionId} ttlMs=${lease.ttlMs} heartbeatMs=${lease.heartbeatIntervalMs}`,
				),
			),
			Effect.map(Option.some),
			Effect.catchTag("VigilDaemonEnsureError", (error) =>
				Effect.logWarning(
					`[vigil] daemon session unavailable; continuing without managed session: ${error.message}`,
				).pipe(Effect.as(Option.none())),
			),
		);

		if (Option.isNone(maybeLease)) {
			return yield* effect;
		}

		return yield* Effect.acquireUseRelease(
			Effect.succeed(maybeLease.value),
			(lease) =>
				Effect.scoped(
					Effect.gen(function* () {
						yield* runDaemonHeartbeatLoop(options, lease).pipe(
							Effect.forkScoped,
						);
						return yield* effect;
					}),
				),
			(lease) =>
				closeDaemonSession(options, lease.sessionId).pipe(
					Effect.catchTag("VigilDaemonEnsureError", (error) =>
						Effect.logWarning(
							`[vigil] daemon session close failed for ${lease.sessionId}: ${error.message}`,
						),
					),
				),
		);
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
			const lifecycle =
				process.env[VIGIL_DAEMON_MANAGED_ENV_VAR] === "1"
					? "managed"
					: "persistent";
			return yield* startVigilServerProgram({
				host: args.serverHost,
				port: args.serverPort,
				daemonToken,
				lifecycle,
			});
		}

		const connectionOptions: DaemonConnectionOptions = {
			host: args.serverHost,
			port: args.serverPort,
			daemonToken,
		};

		yield* ensureServer(connectionOptions);

		yield* withOptionalDaemonSession(
			connectionOptions,
			startVigilTuiProgram({
				chooserFilePath: args.chooserFilePath,
				daemonConnection: {
					host: args.serverHost,
					port: args.serverPort,
					token: daemonToken,
				},
			}),
		);
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
