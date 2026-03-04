import { spawn } from "node:child_process";
import {
	buildVigilDaemonBaseUrl,
	type DaemonUnauthorizedError,
	makeVigilDaemonClient,
	makeVigilDaemonHttpClientLayer,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	type VigilDaemonConnection,
} from "@vigil/api";
import { Data, Effect, Either, Option, Schedule } from "effect";

const DAEMON_POLL_INTERVAL_MS = 100;
const DAEMON_POLL_ATTEMPTS = 50;
const VIGIL_DAEMON_MANAGED_ENV_VAR = "VIGIL_DAEMON_MANAGED";

class DaemonProbeUnreachableError extends Data.TaggedError(
	"DaemonProbeUnreachableError",
)<{
	readonly message: string;
	readonly cause?: unknown;
}> {}

export class DaemonSupervisorError extends Data.TaggedError(
	"DaemonSupervisorError",
)<{
	readonly message: string;
	readonly cause?: unknown;
}> {}

function makeProbeClient(connection: VigilDaemonConnection) {
	return makeVigilDaemonClient(connection).pipe(
		Effect.provide(makeVigilDaemonHttpClientLayer(connection)),
	);
}

function probeDaemon(
	connection: VigilDaemonConnection,
): Effect.Effect<void, DaemonUnauthorizedError | DaemonProbeUnreachableError> {
	return Effect.gen(function* () {
		const daemonClient = yield* makeProbeClient(connection);
		const metaResult = yield* daemonClient.system.meta().pipe(Effect.either);

		if (Either.isLeft(metaResult)) {
			const error = metaResult.left;
			if (error._tag === "DaemonUnauthorizedError") {
				return yield* error;
			}

			return yield* new DaemonProbeUnreachableError({
				message: error.message,
				cause: error,
			});
		}

		const healthResult = yield* daemonClient.system
			.health()
			.pipe(Effect.either);
		if (Either.isLeft(healthResult)) {
			const error = healthResult.left;
			if (error._tag === "DaemonUnauthorizedError") {
				return yield* error;
			}
			return yield* new DaemonProbeUnreachableError({
				message: error.message,
				cause: error,
			});
		}
	});
}

const spawnManagedDaemon = Effect.fn("daemonSupervisor.spawnManagedDaemon")(
	function* (connection: VigilDaemonConnection) {
		const entrypoint = process.argv[1];
		if (!entrypoint) {
			return yield* new DaemonSupervisorError({
				message: "Unable to resolve CLI entrypoint to restart daemon.",
			});
		}

		yield* Effect.try({
			try: () => {
				const processHandle = spawn(
					process.execPath,
					[
						entrypoint,
						"serve",
						"--host",
						connection.host,
						"--port",
						String(connection.port),
					],
					{
						detached: true,
						stdio: "ignore",
						windowsHide: true,
						env: {
							...process.env,
							[VIGIL_DAEMON_TOKEN_ENV_VAR]: connection.token,
							[VIGIL_DAEMON_MANAGED_ENV_VAR]: "1",
						},
					},
				);

				processHandle.unref();
			},
			catch: (cause) =>
				new DaemonSupervisorError({
					message: "Failed to restart managed daemon process.",
					cause,
				}),
		});
	},
);

export const ensureManagedDaemonAvailable = Effect.fn(
	"daemonSupervisor.ensureManagedDaemonAvailable",
)(function* (connection: VigilDaemonConnection) {
	const firstProbeDetail = yield* probeDaemon(connection).pipe(
		Effect.as(Option.none<string>()),
		Effect.catchTag(
			"DaemonUnauthorizedError",
			() =>
				new DaemonSupervisorError({
					message: `A server is already listening at ${buildVigilDaemonBaseUrl(connection)} but rejected this daemon token.`,
				}),
		),
		Effect.catchTag("DaemonProbeUnreachableError", (error) =>
			Effect.succeed(Option.some(error.message)),
		),
	);
	if (Option.isNone(firstProbeDetail)) {
		return;
	}

	yield* spawnManagedDaemon(connection);

	const startupRetrySchedule = Schedule.spaced(
		`${DAEMON_POLL_INTERVAL_MS} millis`,
	).pipe(
		Schedule.compose(Schedule.recurs(DAEMON_POLL_ATTEMPTS - 1)),
		Schedule.whileInput(
			(error: DaemonUnauthorizedError | DaemonProbeUnreachableError) =>
				error._tag === "DaemonProbeUnreachableError",
		),
	);

	yield* probeDaemon(connection).pipe(
		Effect.retry(startupRetrySchedule),
		Effect.catchTag(
			"DaemonUnauthorizedError",
			() =>
				new DaemonSupervisorError({
					message: `Daemon at ${buildVigilDaemonBaseUrl(connection)} rejected this daemon token after restart.`,
				}),
		),
		Effect.catchTag(
			"DaemonProbeUnreachableError",
			(error) =>
				new DaemonSupervisorError({
					message: `Unable to connect to daemon at ${buildVigilDaemonBaseUrl(connection)} after restart. Last check: ${error.message}`,
					cause: error,
				}),
		),
	);
});
