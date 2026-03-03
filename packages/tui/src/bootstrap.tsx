import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { Data, Effect, type Option, pipe } from "effect";
import {
	makeVigilDaemonApiCall,
	type VigilDaemonConnection,
} from "#daemon/client.ts";
import { initializeTreeSitterClient } from "#syntax/tree-sitter.ts";
import {
	loadThemeCatalog,
	readThemePreferenceFromTuiConfig,
	type ThemeCatalog,
} from "#theme/theme.ts";
import { App } from "#ui/app.tsx";

export interface StartVigilTuiOptions {
	readonly chooserFilePath: Option.Option<string>;
	readonly daemonConnection: VigilDaemonConnection;
}

export class ThemeCatalogLoadError extends Data.TaggedError(
	"ThemeCatalogLoadError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

export class ThemePreferenceLoadError extends Data.TaggedError(
	"ThemePreferenceLoadError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

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

export type StartVigilTuiError =
	| ThemeCatalogLoadError
	| ThemePreferenceLoadError
	| RendererCreateError
	| AppRenderError;

function selectInitialThemeName(
	themeCatalog: ThemeCatalog,
	themePreference: Awaited<ReturnType<typeof readThemePreferenceFromTuiConfig>>,
): string {
	if (themePreference.theme && themeCatalog.themes[themePreference.theme]) {
		return themePreference.theme;
	}
	if (themeCatalog.themes["catppuccin-macchiato"]) {
		return "catppuccin-macchiato";
	}
	if (themeCatalog.themes.opencode) {
		return "opencode";
	}
	return themeCatalog.order[0] ?? "opencode";
}

export function startVigilTuiProgram(
	options: StartVigilTuiOptions,
): Effect.Effect<void, StartVigilTuiError> {
	return Effect.gen(function* () {
		const themeCatalog = yield* Effect.tryPromise({
			try: () => loadThemeCatalog(),
			catch: (cause) =>
				new ThemeCatalogLoadError({
					message: "Failed to load theme catalog.",
					cause,
				}),
		});
		const themePreference = yield* Effect.tryPromise({
			try: () => readThemePreferenceFromTuiConfig(),
			catch: (cause) =>
				new ThemePreferenceLoadError({
					message: "Failed to read theme preference configuration.",
					cause,
				}),
		});

		yield* pipe(
			initializeTreeSitterClient(),
			Effect.catchTag("TreeSitterInitializeError", (typedError) =>
				Effect.sync(() => {
					console.error(
						`Failed to initialize Tree-sitter syntax parsers: ${typedError.cause ? String(typedError.cause) : typedError.message}`,
					);
				}),
			),
			Effect.asVoid,
		);

		const initialThemeName = selectInitialThemeName(
			themeCatalog,
			themePreference,
		);
		const daemonApiCall = makeVigilDaemonApiCall(options.daemonConnection);

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
					<App
						themeCatalog={themeCatalog}
						initialThemeName={initialThemeName}
						initialThemeMode={themePreference.mode ?? "dark"}
						chooserFilePath={options.chooserFilePath}
						daemonApiCall={daemonApiCall}
					/>,
				),
			catch: (cause) =>
				new AppRenderError({
					message: "Failed to render vigil UI.",
					cause,
				}),
		});
	});
}
