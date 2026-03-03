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

export class DaemonUnauthorizedError extends Schema.TaggedError<
	DaemonUnauthorizedError
>()(
	"DaemonUnauthorizedError",
	{
		message: Schema.String,
	},
	HttpApiSchema.annotations({ status: 401 }),
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

export class VigilApi extends HttpApi.make("vigil").add(SystemApi) {}
