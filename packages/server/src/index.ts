import { HttpApiBuilder } from "@effect/platform";
import { BunHttpServer } from "@effect/platform-bun";
import { HealthResponse, VigilApi } from "@vigil/api";
import { Cause, Data, Effect, Layer, pipe } from "effect";

export interface StartVigilServerOptions {
	readonly host: string;
	readonly port: number;
}

export class VigilServerStartError extends Data.TaggedError(
	"VigilServerStartError",
)<{
	readonly message: string;
	readonly cause: Cause.Cause<unknown>;
}> {}

const SystemApiLive = HttpApiBuilder.group(VigilApi, "system", (handlers) =>
	handlers.handle("health", () =>
		Effect.succeed(
			HealthResponse.make({
				status: "ok",
			}),
		),
	),
);

const VigilApiLive = HttpApiBuilder.api(VigilApi).pipe(
	Layer.provide(SystemApiLive),
);

function makeServerLive(options: StartVigilServerOptions) {
	return pipe(
		HttpApiBuilder.serve(),
		Layer.provide(VigilApiLive),
		Layer.provide(
			BunHttpServer.layer({
				hostname: options.host,
				port: options.port,
			}),
		),
	);
}

export function startVigilServerProgram(
	options: StartVigilServerOptions,
): Effect.Effect<never, VigilServerStartError> {
	return pipe(
		Layer.launch(makeServerLive(options)),
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
