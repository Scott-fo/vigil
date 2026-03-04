import { Data, Effect, Fiber, Schedule } from "effect";
import { useEffect } from "react";
import {
	type VigilDaemonClient,
	type VigilDaemonConnection,
	VigilDaemonClientContext,
} from "#daemon/client.ts";
import { ensureManagedDaemonAvailable } from "#daemon/supervisor.ts";
import { useFrontendRuntime } from "#runtime/frontend-runtime.tsx";

const MIN_DAEMON_HEARTBEAT_MS = 500;

interface UseDaemonSessionOptions {
	readonly daemonConnection: VigilDaemonConnection;
	readonly enabled: boolean;
	readonly reconnectDelayMs?: number;
}

class DaemonSessionOpenError extends Data.TaggedError("DaemonSessionOpenError")<{
	readonly message: string;
	readonly cause: unknown;
}> {}

class DaemonSessionHeartbeatError extends Data.TaggedError(
	"DaemonSessionHeartbeatError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

class DaemonSessionCloseError extends Data.TaggedError(
	"DaemonSessionCloseError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

const closeDaemonSessionBestEffort = Effect.fn(
	"useDaemonSession.closeDaemonSessionBestEffort",
)(function* (daemonClient: VigilDaemonClient, sessionId: string) {
	yield* daemonClient.session
		.close({
			payload: {
				sessionId,
			},
		})
		.pipe(
			Effect.mapError(
				(cause) =>
					new DaemonSessionCloseError({
						message: `Failed to close daemon session ${sessionId}.`,
						cause,
					}),
			),
			Effect.catchAll(() => Effect.void),
			Effect.asVoid,
		);
});

const runSessionAttempt = Effect.fn("useDaemonSession.runSessionAttempt")(
	function* (daemonClient: VigilDaemonClient) {
		const lease = yield* daemonClient.session.open().pipe(
			Effect.mapError(
				(cause) =>
					new DaemonSessionOpenError({
						message: "Failed to open daemon session.",
						cause,
					}),
			),
		);

		const heartbeatIntervalMs = Math.max(
			MIN_DAEMON_HEARTBEAT_MS,
			lease.heartbeatIntervalMs,
		);

		yield* Effect.forever(
			Effect.sleep(`${heartbeatIntervalMs} millis`).pipe(
				Effect.zipRight(
					daemonClient.session
						.heartbeat({
							payload: {
								sessionId: lease.sessionId,
							},
						})
						.pipe(
							Effect.mapError(
								(cause) =>
									new DaemonSessionHeartbeatError({
										message: `Failed to heartbeat daemon session ${lease.sessionId}.`,
										cause,
									}),
							),
						),
				),
			),
		).pipe(
			Effect.ensuring(
				closeDaemonSessionBestEffort(daemonClient, lease.sessionId),
			),
		);
	},
);

const makeSessionLoop = Effect.fn("useDaemonSession.makeSessionLoop")(function* (
	daemonConnection: VigilDaemonConnection,
	reconnectDelayMs: number,
) {
	const daemonClient = yield* VigilDaemonClientContext;

	yield* runSessionAttempt(daemonClient).pipe(
		Effect.tapError(() =>
			ensureManagedDaemonAvailable(daemonConnection).pipe(
				Effect.catchAll(() => Effect.void),
			),
		),
		Effect.retry(Schedule.spaced(`${reconnectDelayMs} millis`)),
	);
});

export function useDaemonSession(options: UseDaemonSessionOptions) {
	const { daemonConnection, enabled } = options;
	const reconnectDelayMs = options.reconnectDelayMs ?? 1_500;
	const runtime = useFrontendRuntime();

	useEffect(() => {
		if (!enabled) {
			return;
		}

		const fiber = runtime.runFork(
			makeSessionLoop(daemonConnection, reconnectDelayMs),
		);

		return () => {
			runtime.runFork(Fiber.interrupt(fiber));
		};
	}, [daemonConnection, enabled, reconnectDelayMs, runtime]);
}
