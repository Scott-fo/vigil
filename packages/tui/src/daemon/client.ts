import {
	buildVigilDaemonBaseUrl,
	makeVigilDaemonClient,
	type VigilDaemonClient,
	type VigilDaemonConnection,
} from "@vigil/api";
import { Context, Layer } from "effect";

export { buildVigilDaemonBaseUrl, makeVigilDaemonClient };
export type { VigilDaemonClient, VigilDaemonConnection };

export class VigilDaemonClientContext extends Context.Tag(
	"@vigil/tui/VigilDaemonClient",
)<VigilDaemonClientContext, VigilDaemonClient>() {}

export function makeVigilDaemonClientLayer(connection: VigilDaemonConnection) {
	return Layer.effect(VigilDaemonClientContext, makeVigilDaemonClient(connection));
}
