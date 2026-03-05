import { describe, expect, test } from "bun:test";
import { Effect, Either, Stream } from "effect";
import {
	consumeWatchEventStream,
	WATCH_EVENTS_STREAM_IDLE_TIMEOUT_MS,
} from "#daemon/watch-stream.ts";

const textEncoder = new TextEncoder();

function makeReadableByteStream(
	chunks: ReadonlyArray<{
		readonly afterMs: number;
		readonly data: string;
	}>,
): ReadableStream<Uint8Array<ArrayBuffer>> {
	const timeoutHandles: Array<ReturnType<typeof setTimeout>> = [];

	return new ReadableStream({
		start(controller) {
			let elapsedMs = 0;

			for (const chunk of chunks) {
				elapsedMs += chunk.afterMs;
				const timeoutHandle = setTimeout(() => {
					controller.enqueue(textEncoder.encode(chunk.data));
				}, elapsedMs);
				timeoutHandle.unref?.();
				timeoutHandles.push(timeoutHandle);
			}

			const closeHandle = setTimeout(() => {
				controller.close();
			}, elapsedMs + 1);
			closeHandle.unref?.();
			timeoutHandles.push(closeHandle);
		},
		cancel() {
			for (const timeoutHandle of timeoutHandles) {
				clearTimeout(timeoutHandle);
			}
		},
	});
}

function fromReadableByteStream(stream: ReadableStream<Uint8Array<ArrayBuffer>>) {
	return Stream.fromReadableStream(() => stream, (cause) => cause);
}

describe("consumeWatchEventStream", () => {
	test("fails when the stream stays open but no bytes arrive", async () => {
		const outcome = await Effect.runPromise(
			consumeWatchEventStream(
				fromReadableByteStream(new ReadableStream<Uint8Array<ArrayBuffer>>()),
				Effect.void,
				20,
			).pipe(Effect.either),
		);

		expect(Either.isLeft(outcome)).toBe(true);

		if (Either.isLeft(outcome)) {
			expect(outcome.left._tag).toBe("WatchEventsStreamTimedOutError");
			if (outcome.left._tag === "WatchEventsStreamTimedOutError") {
				expect(outcome.left.idleTimeoutMs).toBe(20);
			}
		}
	});

	test("treats keepalive comments as activity and does not refresh", async () => {
		let refreshCount = 0;

		await Effect.runPromise(
			consumeWatchEventStream(
				fromReadableByteStream(
					makeReadableByteStream([
						{ afterMs: 0, data: ": keepalive\n\n" },
						{ afterMs: 5, data: ": keepalive\n\n" },
						{ afterMs: 5, data: ": keepalive\n\n" },
					]),
				),
				Effect.sync(() => {
					refreshCount += 1;
				}),
				20,
			),
		);

		expect(refreshCount).toBe(0);
	});

	test("refreshes when a repo-changed event arrives after keepalive traffic", async () => {
		let refreshCount = 0;

		await Effect.runPromise(
			consumeWatchEventStream(
				fromReadableByteStream(
					makeReadableByteStream([
						{ afterMs: 0, data: ": keepalive\n\n" },
						{
							afterMs: 5,
							data:
								'event: repo-changed\ndata: {"subscriptionId":"sub-1","repoRoot":"/tmp/repo","version":1,"changedAt":"2026-03-05T12:00:00.000Z"}\n\n',
						},
					]),
				),
				Effect.sync(() => {
					refreshCount += 1;
				}),
				WATCH_EVENTS_STREAM_IDLE_TIMEOUT_MS,
			),
		);

		expect(refreshCount).toBe(1);
	});
});
