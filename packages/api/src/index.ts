import {
	HttpApi,
	HttpApiEndpoint,
	HttpApiGroup,
	HttpApiMiddleware,
	HttpApiSchema,
	HttpApiSecurity,
} from "@effect/platform";
import { Schema } from "effect";

export const VIGIL_DAEMON_PROTOCOL_VERSION = 1 as const;
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

export class VigilApi extends HttpApi.make("vigil")
	.add(SystemApi)
	.add(WatchApi) {}
