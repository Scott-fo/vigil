import {
	FetchHttpClient,
	HttpApi,
	HttpApiClient,
	HttpApiEndpoint,
	HttpApiGroup,
	HttpApiMiddleware,
	HttpApiSchema,
	HttpApiSecurity,
	type HttpClient,
} from "@effect/platform";
import { type Effect, Layer, Schema } from "effect";

export const VIGIL_DAEMON_PROTOCOL_VERSION = 2 as const;
export const VIGIL_DAEMON_TOKEN_HEADER = "x-vigil-token" as const;
export const VIGIL_DAEMON_TOKEN_ENV_VAR = "VIGIL_DAEMON_TOKEN" as const;

export class HealthResponse extends Schema.Class<HealthResponse>(
	"HealthResponse",
)({
	status: Schema.Literal("ok"),
}) {}

export class DaemonMetaResponse extends Schema.Class<DaemonMetaResponse>(
	"DaemonMetaResponse",
)({
	name: Schema.Literal("vigil"),
	protocolVersion: Schema.Literal(VIGIL_DAEMON_PROTOCOL_VERSION),
	tokenHeader: Schema.Literal(VIGIL_DAEMON_TOKEN_HEADER),
}) {}

export class WatchSubscribeRequest extends Schema.Class<WatchSubscribeRequest>(
	"WatchSubscribeRequest",
)({
	clientId: Schema.NonEmptyString,
	repoPath: Schema.NonEmptyString,
}) {}

export class WatchSubscribeResponse extends Schema.Class<WatchSubscribeResponse>(
	"WatchSubscribeResponse",
)({
	subscriptionId: Schema.NonEmptyString,
	repoRoot: Schema.NonEmptyString,
	version: Schema.Number,
}) {}

export class WatchUnsubscribeRequest extends Schema.Class<WatchUnsubscribeRequest>(
	"WatchUnsubscribeRequest",
)({
	clientId: Schema.NonEmptyString,
	subscriptionId: Schema.NonEmptyString,
}) {}

export class WatchUnsubscribeAllRequest extends Schema.Class<WatchUnsubscribeAllRequest>(
	"WatchUnsubscribeAllRequest",
)({
	clientId: Schema.NonEmptyString,
}) {}

export class WatchEventsUrlParams extends Schema.Class<WatchEventsUrlParams>(
	"WatchEventsUrlParams",
)({
	clientId: Schema.NonEmptyString,
}) {}

export class SessionOpenResponse extends Schema.Class<SessionOpenResponse>(
	"SessionOpenResponse",
)({
	sessionId: Schema.NonEmptyString,
	heartbeatIntervalMs: Schema.Number,
	ttlMs: Schema.Number,
}) {}

export class SessionHeartbeatRequest extends Schema.Class<SessionHeartbeatRequest>(
	"SessionHeartbeatRequest",
)({
	sessionId: Schema.NonEmptyString,
}) {}

export class SessionCloseRequest extends Schema.Class<SessionCloseRequest>(
	"SessionCloseRequest",
)({
	sessionId: Schema.NonEmptyString,
}) {}

export class ReviewScopeRequest extends Schema.Class<ReviewScopeRequest>(
	"ReviewScopeRequest",
)({
	repoRoot: Schema.NonEmptyString,
	mode: Schema.Literal("working-tree", "branch-compare"),
	sourceRef: Schema.NullOr(Schema.String),
	destinationRef: Schema.NullOr(Schema.String),
	scopeKey: Schema.NonEmptyString,
}) {}

export class ReviewOverallAnchorRequest extends Schema.Class<ReviewOverallAnchorRequest>(
	"ReviewOverallAnchorRequest",
)({
	anchorType: Schema.Literal("overall"),
}) {}

export class ReviewLineAnchorRequest extends Schema.Class<ReviewLineAnchorRequest>(
	"ReviewLineAnchorRequest",
)({
	anchorType: Schema.Literal("line"),
	filePath: Schema.NonEmptyString,
	lineSide: Schema.Literal("old", "new"),
	lineNumber: Schema.Int,
	hunkHeader: Schema.NullOr(Schema.String),
	lineContentHash: Schema.NullOr(Schema.String),
}) {}

export const ReviewThreadAnchorRequest = Schema.Union(
	ReviewOverallAnchorRequest,
	ReviewLineAnchorRequest,
);
export type ReviewThreadAnchorRequest = typeof ReviewThreadAnchorRequest.Type;

export class ReviewListThreadsRequest extends Schema.Class<ReviewListThreadsRequest>(
	"ReviewListThreadsRequest",
)({
	scope: ReviewScopeRequest,
	filePath: Schema.NullOr(Schema.String),
	includeResolved: Schema.Boolean,
	includeStale: Schema.Boolean,
	activeAnchors: Schema.NullOr(Schema.Array(ReviewThreadAnchorRequest)),
}) {}

export class ReviewCreateOverallThreadRequest extends Schema.Class<ReviewCreateOverallThreadRequest>(
	"ReviewCreateOverallThreadRequest",
)({
	scope: ReviewScopeRequest,
	body: Schema.String,
	author: Schema.NullOr(Schema.String),
	threadId: Schema.NullOr(Schema.String),
	commentId: Schema.NullOr(Schema.String),
}) {}

export class ReviewCreateLineThreadRequest extends Schema.Class<ReviewCreateLineThreadRequest>(
	"ReviewCreateLineThreadRequest",
)({
	scope: ReviewScopeRequest,
	anchor: ReviewLineAnchorRequest,
	body: Schema.String,
	author: Schema.NullOr(Schema.String),
	threadId: Schema.NullOr(Schema.String),
	commentId: Schema.NullOr(Schema.String),
}) {}

export class ReviewReplyToThreadRequest extends Schema.Class<ReviewReplyToThreadRequest>(
	"ReviewReplyToThreadRequest",
)({
	scope: ReviewScopeRequest,
	threadId: Schema.NonEmptyString,
	body: Schema.String,
	author: Schema.NullOr(Schema.String),
	commentId: Schema.NullOr(Schema.String),
}) {}

export class ReviewUpdateThreadStateRequest extends Schema.Class<ReviewUpdateThreadStateRequest>(
	"ReviewUpdateThreadStateRequest",
)({
	scope: ReviewScopeRequest,
	threadId: Schema.NonEmptyString,
}) {}

export class ReviewCommentResponse extends Schema.Class<ReviewCommentResponse>(
	"ReviewCommentResponse",
)({
	id: Schema.String,
	threadId: Schema.String,
	author: Schema.String,
	body: Schema.String,
	createdAtMs: Schema.Number,
	updatedAtMs: Schema.Number,
}) {}

export class ReviewThreadResponse extends Schema.Class<ReviewThreadResponse>(
	"ReviewThreadResponse",
)({
	id: Schema.String,
	repoRoot: Schema.String,
	scopeType: Schema.Literal("working-tree", "branch-compare"),
	scopeKey: Schema.String,
	sourceRef: Schema.NullOr(Schema.String),
	destinationRef: Schema.NullOr(Schema.String),
	filePath: Schema.NullOr(Schema.String),
	lineSide: Schema.NullOr(Schema.Literal("old", "new")),
	lineNumber: Schema.NullOr(Schema.Number),
	hunkHeader: Schema.NullOr(Schema.String),
	lineContentHash: Schema.NullOr(Schema.String),
	isResolved: Schema.Boolean,
	createdAtMs: Schema.Number,
	updatedAtMs: Schema.Number,
}) {}

export class ReviewThreadWithCommentsResponse extends Schema.Class<ReviewThreadWithCommentsResponse>(
	"ReviewThreadWithCommentsResponse",
)({
	thread: ReviewThreadResponse,
	comments: Schema.Array(ReviewCommentResponse),
	isStale: Schema.Boolean,
}) {}

export class DaemonUnauthorizedError extends Schema.TaggedError<DaemonUnauthorizedError>()(
	"DaemonUnauthorizedError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 401 }),
) {}

export class WatchBadRequestError extends Schema.TaggedError<WatchBadRequestError>()(
	"WatchBadRequestError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 400 }),
) {}

export class WatchSubscriptionNotFoundError extends Schema.TaggedError<WatchSubscriptionNotFoundError>()(
	"WatchSubscriptionNotFoundError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 404 }),
) {}

export class SessionBadRequestError extends Schema.TaggedError<SessionBadRequestError>()(
	"SessionBadRequestError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 400 }),
) {}

export class SessionNotFoundError extends Schema.TaggedError<SessionNotFoundError>()(
	"SessionNotFoundError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 404 }),
) {}

export class ReviewBadRequestError extends Schema.TaggedError<ReviewBadRequestError>()(
	"ReviewBadRequestError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 400 }),
) {}

export class ReviewNotFoundError extends Schema.TaggedError<ReviewNotFoundError>()(
	"ReviewNotFoundError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 404 }),
) {}

export class VigilDaemonAuth extends HttpApiMiddleware.Tag<VigilDaemonAuth>()(
	"VigilDaemonAuth",
	{
		failure: DaemonUnauthorizedError,
		security: {
			daemonToken: HttpApiSecurity.apiKey({
				in: "header",
				key: VIGIL_DAEMON_TOKEN_HEADER,
			}),
		},
	},
) {}

export class SystemApi extends HttpApiGroup.make("system")
	.add(HttpApiEndpoint.get("health")`/health`.addSuccess(HealthResponse))
	.add(HttpApiEndpoint.get("meta")`/meta`.addSuccess(DaemonMetaResponse))
	.middleware(VigilDaemonAuth) {}

export class WatchApi extends HttpApiGroup.make("watch")
	.add(
		HttpApiEndpoint.post("subscribe")`/watch/subscribe`
			.setPayload(WatchSubscribeRequest)
			.addSuccess(WatchSubscribeResponse)
			.addError(WatchBadRequestError),
	)
	.add(
		HttpApiEndpoint.post("unsubscribe")`/watch/unsubscribe`
			.setPayload(WatchUnsubscribeRequest)
			.addSuccess(HttpApiSchema.NoContent)
			.addError(WatchBadRequestError)
			.addError(WatchSubscriptionNotFoundError),
	)
	.add(
		HttpApiEndpoint.post("unsubscribeAll")`/watch/unsubscribe-all`
			.setPayload(WatchUnsubscribeAllRequest)
			.addSuccess(HttpApiSchema.NoContent)
			.addError(WatchBadRequestError),
	)
	.add(
		HttpApiEndpoint.get("events")`/watch/events`
			.setUrlParams(WatchEventsUrlParams)
			.addSuccess(
				Schema.String.pipe(
					HttpApiSchema.withEncoding({
						kind: "Text",
						contentType: "text/event-stream",
					}),
				),
			)
			.addError(WatchBadRequestError),
	)
	.middleware(VigilDaemonAuth) {}

export class SessionApi extends HttpApiGroup.make("session")
	.add(
		HttpApiEndpoint.post("open")`/session/open`
			.addSuccess(SessionOpenResponse)
			.addError(SessionBadRequestError),
	)
	.add(
		HttpApiEndpoint.post("heartbeat")`/session/heartbeat`
			.setPayload(SessionHeartbeatRequest)
			.addSuccess(HttpApiSchema.NoContent)
			.addError(SessionBadRequestError)
			.addError(SessionNotFoundError),
	)
	.add(
		HttpApiEndpoint.post("close")`/session/close`
			.setPayload(SessionCloseRequest)
			.addSuccess(HttpApiSchema.NoContent)
			.addError(SessionBadRequestError),
	)
	.middleware(VigilDaemonAuth) {}

export class ReviewApi extends HttpApiGroup.make("review")
	.add(
		HttpApiEndpoint.post("listThreads")`/review/threads/list`
			.setPayload(ReviewListThreadsRequest)
			.addSuccess(Schema.Array(ReviewThreadWithCommentsResponse))
			.addError(ReviewBadRequestError)
			.addError(ReviewNotFoundError),
	)
	.add(
		HttpApiEndpoint.post("createOverallThread")`/review/threads/overall`
			.setPayload(ReviewCreateOverallThreadRequest)
			.addSuccess(ReviewThreadWithCommentsResponse)
			.addError(ReviewBadRequestError)
			.addError(ReviewNotFoundError),
	)
	.add(
		HttpApiEndpoint.post("createLineThread")`/review/threads/line`
			.setPayload(ReviewCreateLineThreadRequest)
			.addSuccess(ReviewThreadWithCommentsResponse)
			.addError(ReviewBadRequestError)
			.addError(ReviewNotFoundError),
	)
	.add(
		HttpApiEndpoint.post("replyToThread")`/review/threads/reply`
			.setPayload(ReviewReplyToThreadRequest)
			.addSuccess(ReviewThreadWithCommentsResponse)
			.addError(ReviewBadRequestError)
			.addError(ReviewNotFoundError),
	)
	.add(
		HttpApiEndpoint.post("resolveThread")`/review/threads/resolve`
			.setPayload(ReviewUpdateThreadStateRequest)
			.addSuccess(ReviewThreadResponse)
			.addError(ReviewBadRequestError)
			.addError(ReviewNotFoundError),
	)
	.add(
		HttpApiEndpoint.post("reopenThread")`/review/threads/reopen`
			.setPayload(ReviewUpdateThreadStateRequest)
			.addSuccess(ReviewThreadResponse)
			.addError(ReviewBadRequestError)
			.addError(ReviewNotFoundError),
	)
	.middleware(VigilDaemonAuth) {}

export class VigilApi extends HttpApi.make("vigil")
	.add(SystemApi)
	.add(WatchApi)
	.add(SessionApi)
	.add(ReviewApi) {}

export interface VigilDaemonConnection {
	readonly host: string;
	readonly port: number;
	readonly token: string;
}

export function buildVigilDaemonBaseUrl(
	connection: Pick<VigilDaemonConnection, "host" | "port">,
) {
	const host = connection.host.includes(":")
		? `[${connection.host.replace(/^\[(.*)\]$/, "$1")}]`
		: connection.host;
	return `http://${host}:${connection.port}`;
}

export function makeVigilDaemonHttpClientLayer(
	connection: VigilDaemonConnection,
): Layer.Layer<HttpClient.HttpClient> {
	return FetchHttpClient.layer.pipe(
		Layer.provide(
			Layer.succeed(FetchHttpClient.RequestInit, {
				headers: {
					[VIGIL_DAEMON_TOKEN_HEADER]: connection.token,
				},
			}),
		),
	);
}

export const makeVigilDaemonClient = (connection: VigilDaemonConnection) =>
	HttpApiClient.make(VigilApi, {
		baseUrl: buildVigilDaemonBaseUrl(connection),
	});

export type VigilDaemonClient = Effect.Effect.Success<
	ReturnType<typeof makeVigilDaemonClient>
>;
