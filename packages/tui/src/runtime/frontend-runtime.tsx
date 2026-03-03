import { HttpClient } from "@effect/platform";
import { makeVigilDaemonHttpClientLayer } from "@vigil/api";
import { Layer, ManagedRuntime } from "effect";
import { createContext, useContext, type ReactNode } from "react";
import {
	makeVigilDaemonClientLayer,
	VigilDaemonClientContext,
	type VigilDaemonConnection,
} from "#daemon/client.ts";

function makeFrontendLayer(connection: VigilDaemonConnection) {
	const daemonHttpClientLayer = makeVigilDaemonHttpClientLayer(connection);
	const daemonApiClientLayer = makeVigilDaemonClientLayer(connection).pipe(
		Layer.provide(daemonHttpClientLayer),
	);

	return Layer.merge(daemonHttpClientLayer, daemonApiClientLayer);
}

export type FrontendRuntime = ReturnType<typeof makeFrontendRuntime>;

const FrontendRuntimeContext = createContext<FrontendRuntime | null>(null);

interface FrontendRuntimeProviderProps {
	readonly runtime: FrontendRuntime;
	readonly children: ReactNode;
}

export function makeFrontendRuntime(
	connection: VigilDaemonConnection,
): ManagedRuntime.ManagedRuntime<
	HttpClient.HttpClient | VigilDaemonClientContext,
	never
> {
	return ManagedRuntime.make(makeFrontendLayer(connection));
}

export function FrontendRuntimeProvider(props: FrontendRuntimeProviderProps) {
	return (
		<FrontendRuntimeContext.Provider value={props.runtime}>
			{props.children}
		</FrontendRuntimeContext.Provider>
	);
}

export function useFrontendRuntime(): FrontendRuntime {
	const runtime = useContext(FrontendRuntimeContext);
	if (!runtime) {
		throw new Error(
			"useFrontendRuntime must be used within FrontendRuntimeProvider.",
		);
	}
	return runtime;
}
