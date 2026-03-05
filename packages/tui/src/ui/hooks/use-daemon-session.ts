import { Data, Effect, Fiber, Schedule } from "effect";
import { useEffect, useMemo } from "react";
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
	readonly onDisconnect?: (message: string) => void;
	readonly onReconnect?: () => void;
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

interface DaemonConnectionStatusReporter {
	readonly disconnect: (message: string) => void;
	readonly reconnect: () => void;
}

export function createDaemonConnectionStatusReporter(callbacks: {
	readonly onDisconnect: ((message: string) => void) | undefined;
	readonly onReconnect: (() => void) | undefined;
}): DaemonConnectionStatusReporter {
	let disconnectMessage: string | null = null;

	return {
		disconnect(message) {
			if (disconnectMessage === message) {
				return;
			}

			disconnectMessage = message;
			callbacks.onDisconnect?.(message);
		},
		reconnect() {
			if (disconnectMessage === null) {
				return;
			}

			disconnectMessage = null;
			callbacks.onReconnect?.();
		},
	};
}

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
	function* (
		daemonClient: VigilDaemonClient,
		statusReporter: DaemonConnectionStatusReporter,
	) {
		const lease = yield* daemonClient.session.open().pipe(
			Effect.mapError(
				(cause) =>
					new DaemonSessionOpenError({
						message: "Failed to open daemon session.",
						cause,
					}),
			),
		);

		yield* Effect.sync(() => {
			statusReporter.reconnect();
		});

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

const recoverDaemonConnection = Effect.fn(
	"useDaemonSession.recoverDaemonConnection",
)(function* (
	daemonConnection: VigilDaemonConnection,
	statusReporter: DaemonConnectionStatusReporter,
) {
	yield* Effect.sync(() => {
		statusReporter.disconnect("Disconnected from background daemon. Retrying...");
	});

	yield* ensureManagedDaemonAvailable(daemonConnection).pipe(
		Effect.catchAll((error) =>
			Effect.sync(() => {
				statusReporter.disconnect(error.message);
			}),
		),
	);
});

const makeSessionLoop = Effect.fn("useDaemonSession.makeSessionLoop")(function* (
	daemonConnection: VigilDaemonConnection,
	reconnectDelayMs: number,
	statusReporter: DaemonConnectionStatusReporter,
) {
	const daemonClient = yield* VigilDaemonClientContext;

	yield* runSessionAttempt(daemonClient, statusReporter).pipe(
		Effect.tapError(() =>
			recoverDaemonConnection(daemonConnection, statusReporter),
		),
		Effect.retry(Schedule.spaced(`${reconnectDelayMs} millis`)),
	);
});

export function useDaemonSession(options: UseDaemonSessionOptions) {
	const { daemonConnection, enabled, onDisconnect, onReconnect } = options;
	const reconnectDelayMs = options.reconnectDelayMs ?? 1_500;
	const runtime = useFrontendRuntime();
	const statusReporter = useMemo(
		() =>
			createDaemonConnectionStatusReporter({
				onDisconnect,
				onReconnect,
			}),
		[onDisconnect, onReconnect],
	);

	useEffect(() => {
		if (!enabled) {
			return;
		}

		const fiber = runtime.runFork(
			makeSessionLoop(daemonConnection, reconnectDelayMs, statusReporter),
		);

		return () => {
			runtime.runFork(Fiber.interrupt(fiber));
		};
	}, [daemonConnection, enabled, reconnectDelayMs, runtime, statusReporter]);
}
