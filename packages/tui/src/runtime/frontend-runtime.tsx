import { Layer, ManagedRuntime } from "effect";
import { createContext, useContext, type ReactNode } from "react";

export type FrontendRuntime = ManagedRuntime.ManagedRuntime<never, never>;

const FrontendRuntimeContext = createContext<FrontendRuntime | null>(null);

interface FrontendRuntimeProviderProps {
	readonly runtime: FrontendRuntime;
	readonly children: ReactNode;
}

export function makeFrontendRuntime(): FrontendRuntime {
	return ManagedRuntime.make(Layer.empty);
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
