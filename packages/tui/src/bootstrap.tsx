import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { Data, Effect, type Option } from "effect";
import type { VigilDaemonConnection } from "#daemon/client.ts";
import type { BlameTarget } from "#tui/types.ts";
import {
	FrontendRuntimeProvider,
	makeFrontendRuntime,
} from "#runtime/frontend-runtime.tsx";
import { BootApp } from "#ui/components/boot-app.tsx";

export interface StartVigilTuiOptions {
	readonly chooserFilePath: Option.Option<string>;
	readonly initialBlameTarget: Option.Option<BlameTarget>;
	readonly daemonConnection: VigilDaemonConnection;
}

export class RendererCreateError extends Data.TaggedError(
	"RendererCreateError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

export class AppRenderError extends Data.TaggedError("AppRenderError")<{
	readonly message: string;
	readonly cause: unknown;
}> {}

export type StartVigilTuiError = RendererCreateError | AppRenderError;

export function startVigilTuiProgram(
	options: StartVigilTuiOptions,
): Effect.Effect<void, StartVigilTuiError> {
	return Effect.gen(function* () {
		const frontendRuntime = makeFrontendRuntime(options.daemonConnection);

		const renderer = yield* Effect.tryPromise({
			try: () => createCliRenderer({ useMouse: true }),
			catch: (cause) =>
				new RendererCreateError({
					message: "Failed to initialize terminal renderer.",
					cause,
				}),
		});

		yield* Effect.try({
			try: () =>
				createRoot(renderer).render(
					<FrontendRuntimeProvider runtime={frontendRuntime}>
						<BootApp
							chooserFilePath={options.chooserFilePath}
							initialBlameTarget={options.initialBlameTarget}
							daemonConnection={options.daemonConnection}
						/>
					</FrontendRuntimeProvider>,
				),
			catch: (cause) =>
				new AppRenderError({
					message: "Failed to render vigil UI.",
					cause,
				}),
		});
	});
}
