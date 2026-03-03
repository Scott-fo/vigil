import { HttpApi, HttpApiEndpoint, HttpApiGroup } from "@effect/platform";
import { Schema } from "effect";

export class HealthResponse extends Schema.Class<HealthResponse>(
	"HealthResponse",
)({
	status: Schema.Literal("ok"),
}) {}

export class SystemApi extends HttpApiGroup.make("system").add(
	HttpApiEndpoint.get("health")`/health`.addSuccess(HealthResponse),
) {}

export class VigilApi extends HttpApi.make("vigil").add(SystemApi) {}
