import { VIGIL_DAEMON_TOKEN_HEADER } from "@vigil/api";
import { Data, Effect, Fiber, Option, Schedule } from "effect";
import { useEffect, useMemo } from "react";
import {
	buildVigilDaemonBaseUrl,
	makeVigilDaemonClient,
	type VigilDaemonClient,
	type VigilDaemonConnection,
} from "#daemon/client.ts";
import {
	appendSseChunk,
	drainSseBlocks,
	parseRepoChangedEventBlock,
	type ParseRepoChangedEventBlockError,
} from "#daemon/watch-events.ts";
import { useFrontendRuntime } from "#runtime/frontend-runtime.tsx";

interface UseDaemonWatchOptions {
	readonly daemonConnection: VigilDaemonConnection;
	readonly repoPath: string;
	readonly enabled: boolean;
	readonly onRefreshInstruction: Effect.Effect<void, never, never>;
	readonly reconnectDelayMs?: number;
}

class WatchSubscribeError extends Data.TaggedError("WatchSubscribeError")<{
	readonly message: string;
	readonly cause: unknown;
}> {}

class WatchEventsRequestError extends Data.TaggedError(
	"WatchEventsRequestError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

class WatchEventsStreamStatusError extends Data.TaggedError(
	"WatchEventsStreamStatusError",
)<{
	readonly message: string;
	readonly status: number;
}> {}

class WatchEventsResponseBodyMissingError extends Data.TaggedError(
	"WatchEventsResponseBodyMissingError",
)<{
	readonly message: string;
}> {}

class WatchEventsStreamReadError extends Data.TaggedError(
	"WatchEventsStreamReadError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

class WatchEventsAbortedError extends Data.TaggedError(
	"WatchEventsAbortedError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

class WatchUnsubscribeAllError extends Data.TaggedError(
	"WatchUnsubscribeAllError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

type WatchLoopError =
	| WatchSubscribeError
	| WatchEventsRequestError
	| WatchEventsStreamStatusError
	| WatchEventsResponseBodyMissingError
	| WatchEventsStreamReadError;

const logParseErrorAndContinue = (error: ParseRepoChangedEventBlockError) =>
	Effect.logWarning(
		`[daemon-watch] ignoring malformed event: ${error.message}`,
	).pipe(Effect.as(Option.none()));

const consumeWatchEventStream = Effect.fn(
	"useDaemonWatch.consumeWatchEventStream",
)(function* (
	response: Response,
	onRefreshInstruction: Effect.Effect<void, never, never>,
) {
	const body = response.body;
	if (!body) {
		return yield* new WatchEventsResponseBodyMissingError({
			message: "Watch events response body is missing.",
		});
	}

	const reader = body.getReader();
	const decoder = new TextDecoder();
	let buffer = "";

	while (true) {
		const { done, value } = yield* Effect.tryPromise({
			try: () => reader.read(),
			catch: (cause) =>
				cause instanceof DOMException && cause.name === "AbortError"
					? new WatchEventsAbortedError({
							message: "Watch events stream read aborted.",
							cause,
						})
					: new WatchEventsStreamReadError({
							message: "Failed while reading watch events stream.",
							cause,
						}),
		});

		if (done) {
			return;
		}

		buffer = appendSseChunk(buffer, decoder.decode(value, { stream: true }));
		const drained = drainSseBlocks(buffer);
		buffer = drained.remaining;

		for (const block of drained.blocks) {
			const parsedEvent = yield* parseRepoChangedEventBlock(block).pipe(
				Effect.catchAll(logParseErrorAndContinue),
			);

			if (Option.isNone(parsedEvent)) {
				continue;
			}

			yield* onRefreshInstruction;
		}
	}
});

const runWatchAttempt = Effect.fn("useDaemonWatch.runWatchAttempt")(function* (
	daemonClient: VigilDaemonClient,
	clientId: string,
	daemonConnection: VigilDaemonConnection,
	repoPath: string,
	onRefreshInstruction: Effect.Effect<void, never, never>,
) {
	yield* daemonClient.watch
		.subscribe({
			payload: {
				clientId,
				repoPath,
			},
		})
		.pipe(
			Effect.mapError(
				(cause) =>
					new WatchSubscribeError({
						message: `Failed to subscribe watcher for repo path ${repoPath}.`,
						cause,
					}),
			),
		);

	yield* onRefreshInstruction;

	const controller = new AbortController();
	yield* Effect.gen(function* () {
		const response = yield* Effect.tryPromise({
			try: () =>
				fetch(
					`${buildVigilDaemonBaseUrl(daemonConnection)}/watch/events?clientId=${encodeURIComponent(clientId)}`,
					{
						headers: {
							[VIGIL_DAEMON_TOKEN_HEADER]: daemonConnection.token,
						},
						signal: controller.signal,
					},
				),
			catch: (cause) =>
				cause instanceof DOMException && cause.name === "AbortError"
					? new WatchEventsAbortedError({
							message: "Watch events stream connection aborted.",
							cause,
						})
					: new WatchEventsRequestError({
							message: "Failed to open watch events stream.",
							cause,
						}),
		});

		if (!response.ok) {
			return yield* new WatchEventsStreamStatusError({
				message: `Watch events stream failed with status ${response.status}.`,
				status: response.status,
			});
		}

		yield* consumeWatchEventStream(response, onRefreshInstruction);
	}).pipe(Effect.ensuring(Effect.sync(() => controller.abort())));
});

const makeWatchLoop = Effect.fn("useDaemonWatch.makeWatchLoop")(function* (
	clientId: string,
	daemonConnection: VigilDaemonConnection,
	repoPath: string,
	onRefreshInstruction: Effect.Effect<void, never, never>,
	reconnectDelayMs: number,
) {
	const daemonClient = yield* makeVigilDaemonClient(daemonConnection);
	const unsubscribeAll = daemonClient.watch
		.unsubscribeAll({
			payload: { clientId },
		})
		.pipe(
			Effect.mapError(
				(cause) =>
					new WatchUnsubscribeAllError({
						message: "Failed to unsubscribe watch client during cleanup.",
						cause,
					}),
			),
			Effect.catchAll((error) =>
				Effect.logWarning(`[daemon-watch] ${error.message}`),
			),
			Effect.asVoid,
		);

	yield* runWatchAttempt(
		daemonClient,
		clientId,
		daemonConnection,
		repoPath,
		onRefreshInstruction,
	).pipe(
		Effect.catchTag("WatchEventsAbortedError", () => Effect.interrupt),
		Effect.tapError((error: WatchLoopError) =>
			Effect.logWarning(`[daemon-watch] ${error.message}`),
		),
		Effect.retry(Schedule.spaced(`${reconnectDelayMs} millis`)),
		Effect.ensuring(unsubscribeAll),
	);
});

export function useDaemonWatch(options: UseDaemonWatchOptions) {
	const { daemonConnection, repoPath, enabled, onRefreshInstruction } = options;
	const reconnectDelayMs = options.reconnectDelayMs ?? 1_500;
	const runtime = useFrontendRuntime();
	const clientId = useMemo(() => `vigil-tui-${crypto.randomUUID()}`, []);

	useEffect(() => {
		if (!enabled) {
			return;
		}

		const fiber = runtime.runFork(
			makeWatchLoop(
				clientId,
				daemonConnection,
				repoPath,
				onRefreshInstruction,
				reconnectDelayMs,
			),
		);

		return () => {
			runtime.runFork(Fiber.interrupt(fiber));
		};
	}, [
		clientId,
		daemonConnection,
		enabled,
		onRefreshInstruction,
		reconnectDelayMs,
		repoPath,
		runtime,
	]);
}
