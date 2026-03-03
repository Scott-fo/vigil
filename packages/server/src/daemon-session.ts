import { Cause, Context, Data, Effect, Fiber, Layer } from "effect";

export const DAEMON_SESSION_TTL_MS = 12_000;
export const DAEMON_SESSION_HEARTBEAT_INTERVAL_MS = 4_000;
export const DAEMON_SESSION_SWEEP_INTERVAL_MS = 2_000;
export const DAEMON_MANAGED_IDLE_GRACE_MS = 15_000;

export interface DaemonSessionLease {
	readonly sessionId: string;
	readonly ttlMs: number;
	readonly heartbeatIntervalMs: number;
}

export interface DaemonSessionLayerOptions {
	readonly sessionTtlMs: number;
	readonly heartbeatIntervalMs: number;
	readonly sweepIntervalMs: number;
	readonly shutdownWhenIdle: boolean;
	readonly idleGraceMs: number;
	readonly onIdle: Effect.Effect<void, never, never>;
}

export class DaemonSessionIdError extends Data.TaggedError(
	"DaemonSessionIdError",
)<{
	readonly message: string;
}> {}

export class DaemonSessionNotFoundError extends Data.TaggedError(
	"DaemonSessionNotFoundError",
)<{
	readonly sessionId: string;
	readonly message: string;
}> {}

function isBlank(value: string): boolean {
	return value.trim().length === 0;
}

export class DaemonSession extends Context.Tag("@vigil/server/DaemonSession")<
	DaemonSession,
	{
		readonly open: () => Effect.Effect<DaemonSessionLease>;
		readonly heartbeat: (
			sessionId: string,
		) => Effect.Effect<void, DaemonSessionIdError | DaemonSessionNotFoundError>;
		readonly close: (
			sessionId: string,
		) => Effect.Effect<void, DaemonSessionIdError>;
	}
>() {
	static readonly layer = (options: DaemonSessionLayerOptions) =>
		Layer.scoped(
			DaemonSession,
			Effect.gen(function* () {
				const lock = yield* Effect.makeSemaphore(1);
				const sessions = new Map<string, number>();
				let idleShutdownFiber: Fiber.RuntimeFiber<void, never> | null = null;

				const withLock = <A, E, R>(effect: Effect.Effect<A, E, R>) =>
					lock.withPermits(1)(effect);

				const scheduleIdleShutdownTimerLocked = Effect.fn(
					"DaemonSession.scheduleIdleShutdownTimerLocked",
				)(function* () {
					if (!options.shutdownWhenIdle) {
						return;
					}
					if (sessions.size > 0 || idleShutdownFiber) {
						return;
					}

					yield* Effect.logInfo(
						`[daemon-session] scheduling idle shutdown in ${options.idleGraceMs}ms`,
					);

					idleShutdownFiber = yield* Effect.sleep(
						`${options.idleGraceMs} millis`,
					).pipe(
						Effect.zipRight(
							withLock(
								Effect.gen(function* () {
									idleShutdownFiber = null;

									if (sessions.size > 0) {
										return;
									}

									yield* Effect.logInfo(
										"[daemon-session] no live sessions remain; requesting managed shutdown",
									);
									yield* options.onIdle;
								}),
							),
						),
						Effect.catchAllCause((cause) =>
							Cause.isInterruptedOnly(cause)
								? Effect.void
								: Effect.logWarning(
										`[daemon-session] idle shutdown timer failed.\n${Cause.pretty(cause)}`,
									),
						),
						Effect.forkDaemon,
					);
				});

				const clearExpiredSessionsLocked = Effect.fn(
					"DaemonSession.clearExpiredSessionsLocked",
				)(function* () {
					const now = Date.now();
					let expiredCount = 0;

					for (const [sessionId, expiresAt] of sessions) {
						if (expiresAt > now) {
							continue;
						}

						sessions.delete(sessionId);
						expiredCount += 1;
					}

					if (expiredCount > 0) {
						yield* Effect.logInfo(
							`[daemon-session] expired ${expiredCount} stale session${expiredCount === 1 ? "" : "s"}`,
						);
					}

					if (sessions.size === 0) {
						yield* scheduleIdleShutdownTimerLocked();
					}
				});

				const open = Effect.fn("DaemonSession.open")(function* () {
					return yield* withLock(
						Effect.gen(function* () {
							yield* clearExpiredSessionsLocked();

							if (idleShutdownFiber) {
								const fiber = idleShutdownFiber;
								idleShutdownFiber = null;
								yield* Fiber.interrupt(fiber);
							}

							const sessionId = crypto.randomUUID();
							sessions.set(sessionId, Date.now() + options.sessionTtlMs);
							yield* Effect.logInfo(
								`[daemon-session] opened sessionId=${sessionId} active=${sessions.size}`,
							);

							return {
								sessionId,
								ttlMs: options.sessionTtlMs,
								heartbeatIntervalMs: options.heartbeatIntervalMs,
							};
						}),
					);
				});

				const heartbeat = Effect.fn("DaemonSession.heartbeat")(function* (
					sessionId: string,
				) {
					const normalizedSessionId = sessionId.trim();
					if (isBlank(normalizedSessionId)) {
						return yield* new DaemonSessionIdError({
							message: "sessionId must not be empty.",
						});
					}

					const updated = yield* withLock(
						Effect.gen(function* () {
							yield* clearExpiredSessionsLocked();

							if (!sessions.has(normalizedSessionId)) {
								return false;
							}

							sessions.set(
								normalizedSessionId,
								Date.now() + options.sessionTtlMs,
							);
							return true;
						}),
					);

					if (!updated) {
						return yield* new DaemonSessionNotFoundError({
							sessionId: normalizedSessionId,
							message: "Session not found.",
						});
					}
				});

				const close = Effect.fn("DaemonSession.close")(function* (
					sessionId: string,
				) {
					const normalizedSessionId = sessionId.trim();
					if (isBlank(normalizedSessionId)) {
						return yield* new DaemonSessionIdError({
							message: "sessionId must not be empty.",
						});
					}

					yield* withLock(
						Effect.gen(function* () {
							yield* clearExpiredSessionsLocked();

							const removed = sessions.delete(normalizedSessionId);
							if (removed) {
								yield* Effect.logInfo(
									`[daemon-session] closed sessionId=${normalizedSessionId} active=${sessions.size}`,
								);
							}

							if (sessions.size === 0) {
								yield* scheduleIdleShutdownTimerLocked();
							}
						}),
					);
				});

				const sweepExpiredSessions = Effect.fn(
					"DaemonSession.sweepExpiredSessions",
				)(function* () {
					yield* withLock(clearExpiredSessionsLocked());
				});

					const sweepFiber = yield* Effect.forever(
						Effect.sleep(`${options.sweepIntervalMs} millis`).pipe(
							Effect.zipRight(sweepExpiredSessions()),
						),
					).pipe(Effect.forkDaemon);

				yield* withLock(scheduleIdleShutdownTimerLocked());

				yield* Effect.addFinalizer(() =>
					Effect.gen(function* () {
						yield* Fiber.interrupt(sweepFiber);

						yield* withLock(
							Effect.gen(function* () {
								sessions.clear();

								if (!idleShutdownFiber) {
									return;
								}

								const fiber = idleShutdownFiber;
								idleShutdownFiber = null;
								yield* Fiber.interrupt(fiber);
							}),
						);
					}),
				);

				return DaemonSession.of({
					open,
					heartbeat,
					close,
				});
			}),
		);
}
