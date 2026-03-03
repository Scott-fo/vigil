import { FetchHttpClient, HttpClient } from "@effect/platform";
import { VIGIL_DAEMON_TOKEN_HEADER } from "@vigil/api";
import { Layer, ManagedRuntime } from "effect";
import { createContext, useContext, type ReactNode } from "react";
import type { VigilDaemonConnection } from "#daemon/client.ts";

function makeFrontendHttpLayer(connection: VigilDaemonConnection) {
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

export type FrontendRuntime = ReturnType<typeof makeFrontendRuntime>;

const FrontendRuntimeContext = createContext<FrontendRuntime | null>(null);

interface FrontendRuntimeProviderProps {
	readonly runtime: FrontendRuntime;
	readonly children: ReactNode;
}

export function makeFrontendRuntime(
	connection: VigilDaemonConnection,
): ManagedRuntime.ManagedRuntime<HttpClient.HttpClient, never> {
	return ManagedRuntime.make(makeFrontendHttpLayer(connection));
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
