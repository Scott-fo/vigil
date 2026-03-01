import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { Data, Effect, pipe } from "effect";
import { App } from "#ui/app";
import {
	loadThemeCatalog,
	readThemePreferenceFromTuiConfig,
	type ThemeCatalog,
} from "#theme/theme";
import {
	initializeTreeSitterClient,
	type TreeSitterInitializeError,
} from "#syntax/tree-sitter";

export interface StartReviewerTuiOptions {
	chooserFilePath?: string;
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

export class RendererCreateError extends Data.TaggedError("RendererCreateError")<{
	readonly message: string;
	readonly cause: unknown;
}> {}

export class AppRenderError extends Data.TaggedError("AppRenderError")<{
	readonly message: string;
	readonly cause: unknown;
}> {}

export type StartReviewerTuiError =
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

function renderTreeSitterCause(error: TreeSitterInitializeError): string {
	const cause = error.cause;
	if (cause instanceof Error) {
		return cause.message;
	}
	return String(cause);
}

export function startReviewerTuiProgram(
	options: StartReviewerTuiOptions,
): Effect.Effect<void, StartReviewerTuiError> {
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
						`Failed to initialize Tree-sitter syntax parsers: ${renderTreeSitterCause(typedError)}`,
					);
				}),
			),
			Effect.asVoid,
		);
		const initialThemeName = selectInitialThemeName(themeCatalog, themePreference);

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
						{...(options.chooserFilePath
							? { chooserFilePath: options.chooserFilePath }
							: {})}
					/>,
				),
			catch: (cause) =>
				new AppRenderError({
					message: "Failed to render reviewer UI.",
					cause,
				}),
		});
	});
}

export async function startReviewerTui(options: StartReviewerTuiOptions = {}) {
	await Effect.runPromise(startReviewerTuiProgram(options));
}
