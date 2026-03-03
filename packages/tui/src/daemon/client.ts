import { HttpApiClient } from "@effect/platform";
import { VigilApi } from "@vigil/api";
import { Effect } from "effect";

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

export const makeVigilDaemonClient = (connection: VigilDaemonConnection) =>
	HttpApiClient.make(VigilApi, {
		baseUrl: buildVigilDaemonBaseUrl(connection),
	});

export type VigilDaemonClient = Effect.Effect.Success<
	ReturnType<typeof makeVigilDaemonClient>
>;
