import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import { loadOrCreateDaemonTokenFromTuiConfig } from "@vigil/config";
import {
	startVigilServerProgram,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	type VigilServerStartError,
} from "@vigil/server";
import {
	ensureManagedDaemonAvailable,
	type StartVigilTuiError,
	startVigilTuiProgram,
} from "@vigil/tui";
import { Data, Effect, pipe } from "effect";
import {
	CliArgumentError,
	parseVigilArgs,
	vigilUsage,
} from "./cli-args";

const VIGIL_DAEMON_MANAGED_ENV_VAR = "VIGIL_DAEMON_MANAGED";

class VigilDaemonEnsureError extends Data.TaggedError(
	"VigilDaemonEnsureError",
)<{
	readonly message: string;
	readonly cause?: unknown;
}> {}

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

		const daemonConnection = {
			host: args.serverHost,
			port: args.serverPort,
			token: daemonToken,
		};

		yield* ensureManagedDaemonAvailable(daemonConnection).pipe(
			Effect.mapError(
				(error) =>
					new VigilDaemonEnsureError({
						message: error.message,
						cause: error.cause,
					}),
			),
		);

		yield* startVigilTuiProgram({
			chooserFilePath: args.chooserFilePath,
			initialBlameTarget: args.initialBlameTarget,
			daemonConnection,
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
