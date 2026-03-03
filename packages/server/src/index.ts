import { HttpApiBuilder } from "@effect/platform";
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
} from "@vigil/api";
import { Cause, Data, Effect, Layer, Redacted, pipe } from "effect";

export {
	DaemonMetaResponse,
	HealthResponse,
	VIGIL_DAEMON_PROTOCOL_VERSION,
	VIGIL_DAEMON_TOKEN_ENV_VAR,
	VIGIL_DAEMON_TOKEN_HEADER,
} from "@vigil/api";

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

function makeVigilApiLive(options: StartVigilServerOptions) {
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

	return HttpApiBuilder.api(VigilApi).pipe(
		Layer.provide(systemApiLive),
		Layer.provide(makeVigilDaemonAuthLayer(options)),
	);
}

function makeServerLive(options: StartVigilServerOptions) {
	return pipe(
		HttpApiBuilder.serve(),
		Layer.provide(makeVigilApiLive(options)),
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
