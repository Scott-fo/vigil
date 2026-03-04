import { HttpApiBuilder, HttpMiddleware, HttpServerResponse } from "@effect/platform";
import { BunHttpServer } from "@effect/platform-bun";
import {
	DaemonMetaResponse,
	DaemonUnauthorizedError,
	HealthResponse,
	ReviewBadRequestError,
	ReviewCommentResponse,
	ReviewNotFoundError,
	ReviewThreadResponse,
	ReviewThreadWithCommentsResponse,
	SupportBadRequestError,
	SupportReviewDiffResponse,
	type ReviewThreadAnchorRequest as ReviewThreadAnchorApi,
	SessionBadRequestError,
	SessionNotFoundError,
	SessionOpenResponse,
	VIGIL_DAEMON_PROTOCOL_VERSION,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	VIGIL_DAEMON_TOKEN_HEADER,
	VigilApi,
	VigilDaemonAuth,
	WatchBadRequestError,
	WatchSubscribeResponse,
	WatchSubscriptionNotFoundError,
} from "@vigil/api";
import { Cause, Data, Deferred, Effect, Layer, Option, Redacted, Stream, pipe } from "effect";
import { DbService } from "./db/service.ts";
import {
	DaemonSession,
	DaemonSessionIdError,
	DaemonSessionNotFoundError,
	DAEMON_MANAGED_IDLE_GRACE_MS,
	DAEMON_SESSION_HEARTBEAT_INTERVAL_MS,
	DAEMON_SESSION_SWEEP_INTERVAL_MS,
	DAEMON_SESSION_TTL_MS,
} from "./daemon-session.ts";
import { RepoSubscription } from "./repo-subscription.ts";
import { RepoWatcher } from "./repo-watcher.ts";
import {
	ReviewCommentNotFoundError,
	ReviewCommentRepository,
	ReviewThreadNotFoundError,
	ReviewThreadRepository,
} from "./repositories/index.ts";
import {
	ReviewScopeValidationError,
	ReviewService,
	ReviewServiceScopeMismatchError,
	ReviewServiceValidationError,
	type ReviewScope,
	type ReviewServiceError,
	type ThreadAnchor,
	type ThreadWithComments,
} from "./review/index.ts";
import {
	SupportService,
	type SupportServiceError,
} from "./support/service.ts";
export {
	RepoWatcher,
	type RepoWatcherEvent,
	type RepoWatcherLease,
	type RepoWatcherRetainError,
	RepoWatcherGitError,
	RepoWatcherResolveError,
} from "./repo-watcher.ts";
export {
	DaemonSession,
	type DaemonSessionLease,
	type DaemonSessionLayerOptions,
	DaemonSessionIdError,
	DaemonSessionNotFoundError,
	DAEMON_MANAGED_IDLE_GRACE_MS,
	DAEMON_SESSION_HEARTBEAT_INTERVAL_MS,
	DAEMON_SESSION_SWEEP_INTERVAL_MS,
	DAEMON_SESSION_TTL_MS,
} from "./daemon-session.ts";

export {
	RepoSubscription,
	type RepoSubscriptionEvent,
	type RepoSubscriptionLease,
	type RepoSubscriptionSubscribeError,
	type RepoSubscriptionUnsubscribeError,
	RepoSubscriptionClientIdError,
	RepoSubscriptionNotFoundError,
} from "./repo-subscription.ts";

export {
	DbError,
	type Db,
	DbService,
	type DbServiceShape,
} from "./db/service.ts";

export {
	buildVigilDaemonBaseUrl,
	DaemonUnauthorizedError,
	DaemonMetaResponse,
	HealthResponse,
	makeVigilDaemonClient,
	makeVigilDaemonHttpClientLayer,
	type VigilDaemonClient,
	type VigilDaemonConnection,
	VIGIL_DAEMON_PROTOCOL_VERSION,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	VIGIL_DAEMON_TOKEN_HEADER,
} from "@vigil/api";

export {
	ReviewComment,
	ReviewLineSideSchema,
	type ReviewLineSide,
	ReviewScopeTypeSchema,
	type ReviewScopeType,
	ReviewThread,
} from "./models/index.ts";

export {
	type CreateReviewCommentInput,
	type CreateReviewThreadInput,
	type ListReviewThreadsOptions,
	ReviewCommentDecodeError,
	ReviewCommentNotFoundError,
	ReviewCommentRepository,
	type ReviewCommentRepositoryError,
	ReviewThreadDecodeError,
	ReviewThreadNotFoundError,
	ReviewThreadRepository,
	type ReviewThreadRepositoryError,
} from "./repositories/index.ts";

export {
	buildBranchCompareScopeKey,
	buildThreadAnchorKey,
	buildWorkingTreeScopeKey,
	type CreateLineThreadInput,
	type CreateOverallThreadInput,
	type ListThreadsInput,
	createBranchCompareScope,
	createOverallAnchor,
	createWorkingTreeScope,
	type ReplyToThreadInput,
	ReviewService,
	type ReviewServiceError,
	ReviewServiceScopeMismatchError,
	ReviewServiceValidationError,
	ReviewScopeValidationError,
	type LineThreadAnchor,
	type OverallThreadAnchor,
	type ReviewScope,
	type ThreadAnchor,
	type ThreadWithComments,
	type UpdateThreadStateInput,
} from "./review/index.ts";

export {
	SupportService,
	type SupportServiceError,
	SupportServiceOpencodeError,
	SupportServiceValidationError,
} from "./support/service.ts";

export interface StartVigilServerOptions {
	readonly host: string;
	readonly port: number;
	readonly daemonToken: string;
	readonly lifecycle?: "persistent" | "managed";
}

export class VigilServerStartError extends Data.TaggedError(
	"VigilServerStartError",
)<{
	readonly message: string;
	readonly cause: Cause.Cause<unknown>;
}> {}

interface VigilServerRuntimeOptions {
	readonly onManagedIdle: Effect.Effect<void, never, never>;
}

const defaultRuntimeOptions: VigilServerRuntimeOptions = {
	onManagedIdle: Effect.void,
};

function makeDaemonSessionLayer(
	options: StartVigilServerOptions,
	runtimeOptions: VigilServerRuntimeOptions,
) {
	const managed = options.lifecycle === "managed";

	return DaemonSession.layer({
		sessionTtlMs: DAEMON_SESSION_TTL_MS,
		heartbeatIntervalMs: DAEMON_SESSION_HEARTBEAT_INTERVAL_MS,
		sweepIntervalMs: DAEMON_SESSION_SWEEP_INTERVAL_MS,
		shutdownWhenIdle: managed,
		idleGraceMs: DAEMON_MANAGED_IDLE_GRACE_MS,
		onIdle: managed ? runtimeOptions.onManagedIdle : Effect.void,
	});
}

function makeVigilDaemonAuthLayer(options: StartVigilServerOptions) {
	return Layer.succeed(
		VigilDaemonAuth,
		VigilDaemonAuth.of({
			daemonToken: (token) =>
				Redacted.value(token) === options.daemonToken
					? Effect.void
					: Effect.fail(
							DaemonUnauthorizedError.make({
								message: "Invalid daemon token.",
							}),
						),
		}),
	);
}

function toReviewScope(input: {
	readonly repoRoot: string;
	readonly mode: "working-tree" | "branch-compare";
	readonly sourceRef: string | null;
	readonly destinationRef: string | null;
	readonly scopeKey: string;
}): ReviewScope {
	return {
		repoRoot: input.repoRoot,
		mode: input.mode,
		sourceRef: Option.fromNullable(input.sourceRef),
		destinationRef: Option.fromNullable(input.destinationRef),
		scopeKey: input.scopeKey,
	};
}

function toThreadAnchor(input: ReviewThreadAnchorApi): ThreadAnchor {
	if (input.anchorType === "overall") {
		return {
			anchorType: "overall",
		};
	}

	return {
		anchorType: "line",
		filePath: input.filePath,
		lineSide: input.lineSide,
		lineNumber: input.lineNumber,
		hunkHeader: Option.fromNullable(input.hunkHeader),
		lineContentHash: Option.fromNullable(input.lineContentHash),
	};
}

function toReviewThreadResponse(input: ThreadWithComments["thread"]) {
	return ReviewThreadResponse.make({
		id: input.id,
		repoRoot: input.repoRoot,
		scopeType: input.scopeType,
		scopeKey: input.scopeKey,
		sourceRef: input.sourceRef,
		destinationRef: input.destinationRef,
		filePath: input.filePath,
		lineSide: input.lineSide,
		lineNumber: input.lineNumber,
		hunkHeader: input.hunkHeader,
		lineContentHash: input.lineContentHash,
		isResolved: input.isResolved,
		createdAtMs: input.createdAtMs,
		updatedAtMs: input.updatedAtMs,
	});
}

function toReviewThreadWithCommentsResponse(input: ThreadWithComments) {
	return ReviewThreadWithCommentsResponse.make({
		thread: toReviewThreadResponse(input.thread),
		comments: input.comments.map((comment) =>
			ReviewCommentResponse.make({
				id: comment.id,
				threadId: comment.threadId,
				author: comment.author,
				body: comment.body,
				createdAtMs: comment.createdAtMs,
				updatedAtMs: comment.updatedAtMs,
			}),
		),
		isStale: input.isStale,
	});
}

function mapReviewErrors<A, R>(
	effect: Effect.Effect<A, ReviewServiceError, R>,
): Effect.Effect<A, ReviewBadRequestError | ReviewNotFoundError, R> {
	return effect.pipe(
		Effect.catchTag("ReviewServiceValidationError", (error) =>
			Effect.fail(
				ReviewBadRequestError.make({
					message: error.message,
				}),
			),
		),
		Effect.catchTag("ReviewScopeValidationError", (error) =>
			Effect.fail(
				ReviewBadRequestError.make({
					message: error.message,
				}),
			),
		),
		Effect.catchTag("ReviewServiceScopeMismatchError", (error) =>
			Effect.fail(
				ReviewNotFoundError.make({
					message: error.message,
				}),
			),
		),
		Effect.catchTag("ReviewThreadNotFoundError", (error) =>
			Effect.fail(
				ReviewNotFoundError.make({
					message: error.message,
				}),
			),
		),
		Effect.catchTag("ReviewCommentNotFoundError", (error) =>
			Effect.fail(
				ReviewNotFoundError.make({
					message: error.message,
				}),
			),
		),
		Effect.catchTag("DbError", (error) =>
			Effect.fail(
				ReviewBadRequestError.make({
					message: error.message,
				}),
			),
		),
		Effect.catchTag("ReviewThreadDecodeError", () =>
			Effect.fail(
				ReviewBadRequestError.make({
					message: "Unable to decode review thread data.",
				}),
			),
		),
		Effect.catchTag("ReviewCommentDecodeError", () =>
			Effect.fail(
				ReviewBadRequestError.make({
					message: "Unable to decode review comment data.",
				}),
			),
		),
	);
}

function mapSupportErrors<A, R>(
	effect: Effect.Effect<A, SupportServiceError, R>,
): Effect.Effect<A, SupportBadRequestError, R> {
	return effect.pipe(
		Effect.catchTag("SupportServiceValidationError", (error) =>
			Effect.fail(
				SupportBadRequestError.make({
					message: error.message,
				}),
			),
		),
		Effect.catchTag("SupportServiceOpencodeError", (error) =>
			Effect.fail(
				SupportBadRequestError.make({
					message: error.message,
				}),
			),
		),
	);
}

export function makeVigilApiLayer(
	options: StartVigilServerOptions,
	runtimeOptions: VigilServerRuntimeOptions = defaultRuntimeOptions,
) {
	const textEncoder = new TextEncoder();
	const encodeSseChunk = (eventName: string, payload: unknown) =>
		textEncoder.encode(`event: ${eventName}\ndata: ${JSON.stringify(payload)}\n\n`);
	const encodeSseComment = (message: string) =>
		textEncoder.encode(`: ${message}\n\n`);
	const keepaliveStream = Stream.repeatEffect(
		Effect.sleep("5 seconds").pipe(Effect.as(encodeSseComment("keepalive"))),
	);

	const systemApiLive = HttpApiBuilder.group(VigilApi, "system", (handlers) =>
		handlers
			.handle("health", () =>
				Effect.succeed(
					HealthResponse.make({
						status: "ok",
					}),
				),
			)
			.handle("meta", () =>
				Effect.succeed(
					DaemonMetaResponse.make({
						name: "vigil",
						protocolVersion: VIGIL_DAEMON_PROTOCOL_VERSION,
						tokenHeader: VIGIL_DAEMON_TOKEN_HEADER,
					}),
				),
			),
	);
	const watchApiLive = HttpApiBuilder.group(VigilApi, "watch", (handlers) =>
		handlers
			.handle("subscribe", ({ payload }) =>
				pipe(
					RepoSubscription,
					Effect.flatMap((subscription) =>
						subscription.subscribe(payload.clientId, payload.repoPath),
					),
					Effect.map((lease) =>
						WatchSubscribeResponse.make({
							subscriptionId: lease.subscriptionId,
							repoRoot: lease.repoRoot,
							version: lease.version,
						}),
					),
					Effect.catchAll((error) =>
						Effect.fail(
							WatchBadRequestError.make({
								message: error.message,
							}),
						),
					),
				),
			)
			.handle("unsubscribe", ({ payload }) =>
				pipe(
					RepoSubscription,
					Effect.flatMap((subscription) =>
						subscription.unsubscribe(payload.clientId, payload.subscriptionId),
					),
					Effect.asVoid,
					Effect.catchTags({
						RepoSubscriptionNotFoundError: (error) =>
							Effect.fail(
								WatchSubscriptionNotFoundError.make({
									message: error.message,
								}),
							),
						RepoSubscriptionClientIdError: (error) =>
							Effect.fail(
								WatchBadRequestError.make({
									message: error.message,
								}),
							),
					}),
				),
			)
			.handle("unsubscribeAll", ({ payload }) =>
				pipe(
					RepoSubscription,
					Effect.flatMap((subscription) =>
						subscription.unsubscribeAll(payload.clientId),
					),
					Effect.asVoid,
				),
			)
			.handle("events", ({ urlParams }) =>
				pipe(
					RepoSubscription,
					Effect.flatMap((subscription) => subscription.events(urlParams.clientId)),
					Effect.map((events) =>
						HttpServerResponse.stream(
							Stream.concat(
								Stream.fromIterable([encodeSseComment("connected")]),
								Stream.merge(
									events.pipe(
										Stream.map((event) =>
											encodeSseChunk("repo-changed", {
												subscriptionId: event.subscriptionId,
												repoRoot: event.repoRoot,
												version: event.version,
												changedAt: event.changedAt.toISOString(),
											}),
										),
									),
									keepaliveStream,
								),
							),
							{
								contentType: "text/event-stream",
								headers: {
									"cache-control": "no-cache",
									connection: "keep-alive",
									"x-accel-buffering": "no",
								},
							},
						),
					),
					Effect.catchAll((error) =>
						Effect.fail(
							WatchBadRequestError.make({
								message: error.message,
							}),
						),
					),
				),
			),
	);
	const sessionApiLive = HttpApiBuilder.group(VigilApi, "session", (handlers) =>
		handlers
			.handle("open", () =>
				pipe(
					DaemonSession,
					Effect.flatMap((session) => session.open()),
					Effect.map((lease) =>
						SessionOpenResponse.make({
							sessionId: lease.sessionId,
							heartbeatIntervalMs: lease.heartbeatIntervalMs,
							ttlMs: lease.ttlMs,
						}),
					),
				),
			)
			.handle("heartbeat", ({ payload }) =>
				pipe(
					DaemonSession,
					Effect.flatMap((session) => session.heartbeat(payload.sessionId)),
					Effect.asVoid,
					Effect.catchTags({
						DaemonSessionIdError: (error) =>
							Effect.fail(
								SessionBadRequestError.make({
									message: error.message,
								}),
							),
						DaemonSessionNotFoundError: (error) =>
							Effect.fail(
								SessionNotFoundError.make({
									message: error.message,
								}),
							),
					}),
				),
			)
			.handle("close", ({ payload }) =>
				pipe(
					DaemonSession,
					Effect.flatMap((session) => session.close(payload.sessionId)),
					Effect.asVoid,
					Effect.catchTag("DaemonSessionIdError", (error) =>
						Effect.fail(
							SessionBadRequestError.make({
								message: error.message,
							}),
						),
					),
				),
			),
	);
	const reviewApiLive = HttpApiBuilder.group(VigilApi, "review", (handlers) =>
		handlers
			.handle("listThreads", ({ payload }) =>
				pipe(
					ReviewService,
					Effect.flatMap((reviewService) =>
						reviewService.listThreads({
							scope: toReviewScope(payload.scope),
							includeResolved: payload.includeResolved,
							includeStale: payload.includeStale,
							...(payload.filePath === null ? {} : { filePath: payload.filePath }),
							...(payload.activeAnchors === null
								? {}
								: {
										activeAnchors: payload.activeAnchors.map((anchor) =>
											toThreadAnchor(anchor),
										),
									}),
						}),
					),
					Effect.map((threads) =>
						threads.map((thread) => toReviewThreadWithCommentsResponse(thread)),
					),
					mapReviewErrors,
				),
			)
			.handle("createOverallThread", ({ payload }) =>
				pipe(
					ReviewService,
					Effect.flatMap((reviewService) =>
						reviewService.createOverallThread({
							scope: toReviewScope(payload.scope),
							body: payload.body,
							...(payload.author === null ? {} : { author: payload.author }),
							...(payload.threadId === null ? {} : { threadId: payload.threadId }),
							...(payload.commentId === null
								? {}
								: { commentId: payload.commentId }),
						}),
					),
					Effect.map((thread) => toReviewThreadWithCommentsResponse(thread)),
					mapReviewErrors,
				),
			)
			.handle("createLineThread", ({ payload }) =>
				pipe(
					ReviewService,
					Effect.flatMap((reviewService) =>
						reviewService.createLineThread({
							scope: toReviewScope(payload.scope),
							anchor: {
								anchorType: "line",
								filePath: payload.anchor.filePath,
								lineSide: payload.anchor.lineSide,
								lineNumber: payload.anchor.lineNumber,
								hunkHeader: Option.fromNullable(payload.anchor.hunkHeader),
								lineContentHash: Option.fromNullable(
									payload.anchor.lineContentHash,
								),
							},
							body: payload.body,
							...(payload.author === null ? {} : { author: payload.author }),
							...(payload.threadId === null ? {} : { threadId: payload.threadId }),
							...(payload.commentId === null
								? {}
								: { commentId: payload.commentId }),
						}),
					),
					Effect.map((thread) => toReviewThreadWithCommentsResponse(thread)),
					mapReviewErrors,
				),
			)
			.handle("replyToThread", ({ payload }) =>
				pipe(
					ReviewService,
					Effect.flatMap((reviewService) =>
						reviewService.replyToThread({
							scope: toReviewScope(payload.scope),
							threadId: payload.threadId,
							body: payload.body,
							...(payload.author === null ? {} : { author: payload.author }),
							...(payload.commentId === null
								? {}
								: { commentId: payload.commentId }),
						}),
					),
					Effect.map((thread) => toReviewThreadWithCommentsResponse(thread)),
					mapReviewErrors,
				),
			)
			.handle("resolveThread", ({ payload }) =>
				pipe(
					ReviewService,
					Effect.flatMap((reviewService) =>
						reviewService.resolveThread({
							scope: toReviewScope(payload.scope),
							threadId: payload.threadId,
						}),
					),
					Effect.map((thread) => toReviewThreadResponse(thread)),
					mapReviewErrors,
				),
			)
			.handle("reopenThread", ({ payload }) =>
				pipe(
					ReviewService,
					Effect.flatMap((reviewService) =>
						reviewService.reopenThread({
							scope: toReviewScope(payload.scope),
							threadId: payload.threadId,
						}),
					),
					Effect.map((thread) => toReviewThreadResponse(thread)),
					mapReviewErrors,
				),
			),
	);
	const supportApiLive = HttpApiBuilder.group(VigilApi, "support", (handlers) =>
		handlers.handle("reviewDiff", ({ payload }) =>
			pipe(
				SupportService,
				Effect.flatMap((supportService) =>
					supportService.reviewDiff({
						repoRoot: payload.repoRoot,
						mode: payload.mode,
						sourceRef: Option.fromNullable(payload.sourceRef),
						destinationRef: Option.fromNullable(payload.destinationRef),
					}),
				),
				Effect.map((markdown) =>
					SupportReviewDiffResponse.make({
						markdown,
					}),
				),
				mapSupportErrors,
			),
		),
	);

	const reviewServiceLive = ReviewService.layer.pipe(
		Layer.provide(ReviewThreadRepository.layer),
		Layer.provide(ReviewCommentRepository.layer),
		Layer.provide(DbService.layer),
		Layer.orDie,
	);

	return HttpApiBuilder.api(VigilApi).pipe(
		Layer.provide(systemApiLive),
		Layer.provide(watchApiLive),
		Layer.provide(sessionApiLive),
		Layer.provide(reviewApiLive),
		Layer.provide(supportApiLive),
		Layer.provide(makeVigilDaemonAuthLayer(options)),
		Layer.provide(makeDaemonSessionLayer(options, runtimeOptions)),
		Layer.provide(RepoSubscription.layer),
		Layer.provide(RepoWatcher.layer),
		Layer.provide(reviewServiceLive),
		Layer.provide(SupportService.layer),
	);
}

function makeServerLive(
	options: StartVigilServerOptions,
	runtimeOptions: VigilServerRuntimeOptions = defaultRuntimeOptions,
) {
	return pipe(
		HttpApiBuilder.serve(HttpMiddleware.logger),
		Layer.provide(makeVigilApiLayer(options, runtimeOptions)),
		Layer.provide(
			BunHttpServer.layer({
				hostname: options.host,
				port: options.port,
				idleTimeout: 255,
			}),
		),
	);
}

export function startVigilServerProgram(
	options: StartVigilServerOptions,
): Effect.Effect<void, VigilServerStartError> {
	const startServer = Effect.gen(function* () {
		if (options.lifecycle !== "managed") {
			return yield* Layer.launch(makeServerLive(options));
		}

		const managedIdleSignal = yield* Deferred.make<void>();
		const runtimeOptions: VigilServerRuntimeOptions = {
			onManagedIdle: Deferred.succeed(managedIdleSignal, undefined).pipe(
				Effect.asVoid,
			),
		};

		yield* Effect.logInfo("[vigil-server] managed lifecycle enabled");
		yield* Layer.launch(makeServerLive(options, runtimeOptions)).pipe(
			Effect.raceFirst(Deferred.await(managedIdleSignal)),
		);
	});

	return pipe(
		Effect.logInfo(
			`[vigil-server] starting host=${options.host} port=${options.port}`,
		),
		Effect.zipRight(startServer),
		Effect.catchAllCause((cause) =>
			Effect.fail(
				new VigilServerStartError({
					message: `Failed to start server on ${options.host}:${options.port}.\n${Cause.pretty(cause)}`,
					cause,
				}),
			),
		),
	);
}
