import * as Sse from "@effect/experimental/Sse";
import { Data, Effect, Option, Stream } from "effect";
import { parseRepoChangedSseEvent } from "#daemon/watch-events.ts";

export const WATCH_EVENTS_STREAM_IDLE_TIMEOUT_MS = 15_000;

export class WatchEventsStreamTimedOutError extends Data.TaggedError(
	"WatchEventsStreamTimedOutError",
)<{
	readonly message: string;
	readonly idleTimeoutMs: number;
}> {}

export class WatchEventsStreamReadError extends Data.TaggedError(
	"WatchEventsStreamReadError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

const toWatchEventsStreamReadError = (cause: unknown) =>
	new WatchEventsStreamReadError({
		message: "Failed while reading watch events stream.",
		cause,
	});

export const consumeWatchEventStream = Effect.fn(
	"watchStream.consumeWatchEventStream",
)(function* (
	stream: Stream.Stream<Uint8Array<ArrayBufferLike>, unknown>,
	onRefreshInstruction: Effect.Effect<void, never, never>,
	idleTimeoutMs = WATCH_EVENTS_STREAM_IDLE_TIMEOUT_MS,
) {
	const maybeTimeoutError = yield* stream.pipe(
		Stream.mapError(toWatchEventsStreamReadError),
		Stream.timeoutFail(
			() =>
				new WatchEventsStreamTimedOutError({
					message: `Watch events stream went idle for ${idleTimeoutMs}ms.`,
					idleTimeoutMs,
				}),
			`${idleTimeoutMs} millis`,
		),
		Stream.decodeText(),
		Stream.pipeThroughChannel(Sse.makeChannel()),
		Stream.runForEach((sseEvent) =>
			parseRepoChangedSseEvent(sseEvent).pipe(
				Effect.catchAll(() => Effect.succeedNone),
				Effect.flatMap((parsedEvent) =>
					Option.isSome(parsedEvent) ? onRefreshInstruction : Effect.void,
				),
			),
		),
		Effect.as(Option.none<WatchEventsStreamTimedOutError>()),
		Effect.catchTag("WatchEventsStreamTimedOutError", (error) =>
			Effect.succeed(Option.some(error)),
		),
	);

	if (Option.isSome(maybeTimeoutError)) {
		return yield* maybeTimeoutError.value;
	}
});
