import { HttpApiBuilder, HttpMiddleware, HttpServerResponse } from "@effect/platform";
import { BunHttpServer } from "@effect/platform-bun";
import {
	DaemonMetaResponse,
	DaemonUnauthorizedError,
	HealthResponse,
	VIGIL_DAEMON_PROTOCOL_VERSION,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	VIGIL_DAEMON_TOKEN_HEADER,
	VigilApi,
	VigilDaemonAuth,
	WatchBadRequestError,
	WatchSubscribeResponse,
	WatchSubscriptionNotFoundError,
} from "@vigil/api";
import { Cause, Data, Effect, Layer, Redacted, Stream, pipe } from "effect";
import { DbService } from "./db/service.ts";
import { RepoSubscription } from "./repo-subscription.ts";
import { RepoWatcher } from "./repo-watcher.ts";
export {
	RepoWatcher,
	type RepoWatcherEvent,
	type RepoWatcherLease,
	type RepoWatcherRetainError,
	RepoWatcherGitError,
	RepoWatcherResolveError,
} from "./repo-watcher.ts";
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
	DaemonMetaResponse,
	HealthResponse,
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

export interface StartVigilServerOptions {
	readonly host: string;
	readonly port: number;
	readonly daemonToken: string;
}

export class VigilServerStartError extends Data.TaggedError(
	"VigilServerStartError",
)<{
	readonly message: string;
	readonly cause: Cause.Cause<unknown>;
}> {}

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

export function makeVigilApiLayer(options: StartVigilServerOptions) {
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

	return HttpApiBuilder.api(VigilApi).pipe(
		Layer.provide(systemApiLive),
		Layer.provide(watchApiLive),
		Layer.provide(makeVigilDaemonAuthLayer(options)),
		Layer.provide(RepoSubscription.layer),
		Layer.provide(RepoWatcher.layer),
	);
}

function makeServerLive(options: StartVigilServerOptions) {
	return pipe(
		HttpApiBuilder.serve(HttpMiddleware.logger),
		Layer.provide(makeVigilApiLayer(options)),
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
): Effect.Effect<never, VigilServerStartError> {
	const initializeDatabase = Effect.gen(function* () {
		yield* DbService;
	}).pipe(Effect.provide(DbService.layer));

	return pipe(
		Effect.logInfo(
			`[vigil-server] starting host=${options.host} port=${options.port}`,
		),
		Effect.zipRight(Effect.scoped(initializeDatabase)),
		Effect.zipRight(Layer.launch(makeServerLive(options))),
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
