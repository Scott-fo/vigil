import { FetchHttpClient, HttpApiClient } from "@effect/platform";
import { VIGIL_DAEMON_TOKEN_HEADER, VigilApi } from "@vigil/api";
import { Effect, Layer } from "effect";

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

function makeFetchWithDaemonToken(connection: VigilDaemonConnection) {
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
	}).pipe(Effect.provide(makeFetchWithDaemonToken(connection)));

export type VigilDaemonClient = Effect.Effect.Success<
	ReturnType<typeof makeVigilDaemonClient>
>;
