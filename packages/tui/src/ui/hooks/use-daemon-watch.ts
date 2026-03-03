import * as Sse from "@effect/experimental/Sse";
import { HttpClient, type HttpClientResponse } from "@effect/platform";
import { Data, Effect, Fiber, Option, Schedule, Stream } from "effect";
import { useEffect, useMemo } from "react";
import {
	buildVigilDaemonBaseUrl,
	type VigilDaemonClient,
	VigilDaemonClientContext,
	type VigilDaemonConnection,
} from "#daemon/client.ts";
import {
	type ParseRepoChangedSseEventError,
	parseRepoChangedSseEvent,
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

class WatchEventsStreamReadError extends Data.TaggedError(
	"WatchEventsStreamReadError",
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

const logParseErrorAndContinue = (error: ParseRepoChangedSseEventError) =>
	Effect.logWarning(
		`[daemon-watch] ignoring malformed event: ${error.message}`,
	).pipe(Effect.as(Option.none()));

const consumeWatchEventStream = Effect.fn(
	"useDaemonWatch.consumeWatchEventStream",
)(function* (
	response: HttpClientResponse.HttpClientResponse,
	onRefreshInstruction: Effect.Effect<void, never, never>,
) {
	yield* response.stream.pipe(
		Stream.decodeText(),
		Stream.pipeThroughChannel(Sse.makeChannel()),
		Stream.runForEach((sseEvent) =>
			parseRepoChangedSseEvent(sseEvent).pipe(
				Effect.catchAll(logParseErrorAndContinue),
				Effect.flatMap((parsedEvent) =>
					Option.isSome(parsedEvent) ? onRefreshInstruction : Effect.void,
				),
			),
		),
		Effect.mapError((error) =>
			Effect.fail(
				new WatchEventsStreamReadError({
					message: "Failed while reading watch events stream.",
					cause: error,
				}),
			),
		),
	);
});

const openWatchEventsResponse = Effect.fn(
	"useDaemonWatch.openWatchEventsResponse",
)(function* (
	daemonHttpClient: HttpClient.HttpClient,
	daemonConnection: VigilDaemonConnection,
	clientId: string,
) {
	return yield* daemonHttpClient
		.get(
			`${buildVigilDaemonBaseUrl(daemonConnection)}/watch/events?clientId=${encodeURIComponent(clientId)}`,
		)
		.pipe(
			Effect.mapError(
				(error) =>
					new WatchEventsRequestError({
						message: "Failed to open watch events stream.",
						cause: error,
					}),
			),
		);
});

const runWatchAttempt = Effect.fn("useDaemonWatch.runWatchAttempt")(function* (
	daemonClient: VigilDaemonClient,
	daemonHttpClient: HttpClient.HttpClient,
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

	const response = yield* openWatchEventsResponse(
		daemonHttpClient,
		daemonConnection,
		clientId,
	);

	if (response.status < 200 || response.status >= 300) {
		return yield* new WatchEventsStreamStatusError({
			message: `Watch events stream failed with status ${response.status}.`,
			status: response.status,
		});
	}

	yield* consumeWatchEventStream(response, onRefreshInstruction);
});

const makeWatchLoop = Effect.fn("useDaemonWatch.makeWatchLoop")(function* (
	clientId: string,
	daemonConnection: VigilDaemonConnection,
	repoPath: string,
	onRefreshInstruction: Effect.Effect<void, never, never>,
	reconnectDelayMs: number,
) {
	const daemonClient = yield* VigilDaemonClientContext;
	const daemonHttpClient = yield* HttpClient.HttpClient;

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
		daemonHttpClient,
		clientId,
		daemonConnection,
		repoPath,
		onRefreshInstruction,
	).pipe(
		Effect.tapError((error) =>
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
