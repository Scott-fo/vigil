import { describe, expect, test } from "bun:test";
import { Effect, Ref } from "effect";
import { DaemonSession } from "./daemon-session.ts";

describe("daemon session", () => {
	test("open returns lease metadata", async () => {
		const lease = await Effect.runPromise(
			Effect.gen(function* () {
				const session = yield* DaemonSession;
				const lease = yield* session.open();
				yield* session.close(lease.sessionId);
				return lease;
			}).pipe(
				Effect.provide(
					DaemonSession.layer({
						sessionTtlMs: 120,
						heartbeatIntervalMs: 40,
						sweepIntervalMs: 20,
						shutdownWhenIdle: false,
						idleGraceMs: 100,
						onIdle: Effect.void,
					}),
				),
			),
		);

		expect(lease.sessionId.length).toBeGreaterThan(0);
		expect(lease.ttlMs).toBe(120);
		expect(lease.heartbeatIntervalMs).toBe(40);
	});

	test("heartbeat fails after ttl expiration", async () => {
		const outcome = await Effect.runPromise(
			Effect.gen(function* () {
				const session = yield* DaemonSession;
				const lease = yield* session.open();
				yield* Effect.sleep("140 millis");

				return yield* session.heartbeat(lease.sessionId).pipe(
					Effect.match({
						onFailure: (error) => error._tag,
						onSuccess: () => "ok",
					}),
				);
			}).pipe(
				Effect.provide(
					DaemonSession.layer({
						sessionTtlMs: 60,
						heartbeatIntervalMs: 20,
						sweepIntervalMs: 10,
						shutdownWhenIdle: false,
						idleGraceMs: 100,
						onIdle: Effect.void,
					}),
				),
			),
		);

		expect(outcome).toBe("DaemonSessionNotFoundError");
	});

	test("managed mode requests idle shutdown when last session closes", async () => {
		const idleRequests = await Effect.runPromise(
			Effect.gen(function* () {
				const idleRequestCount = yield* Ref.make(0);
				const layer = DaemonSession.layer({
					sessionTtlMs: 120,
					heartbeatIntervalMs: 40,
					sweepIntervalMs: 20,
					shutdownWhenIdle: true,
					idleGraceMs: 100,
					onIdle: Ref.update(idleRequestCount, (count) => count + 1),
				});

				yield* Effect.gen(function* () {
					const session = yield* DaemonSession;
					const lease = yield* session.open();
					yield* session.close(lease.sessionId);
					yield* Effect.sleep("160 millis");
				}).pipe(Effect.provide(layer));

				return yield* Ref.get(idleRequestCount);
			}),
		);

		expect(idleRequests).toBe(1);
	});
});
