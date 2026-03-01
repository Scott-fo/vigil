import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { Effect, pipe } from "effect";
import { App } from "#ui/app";
import {
	loadThemeCatalog,
	readThemePreferenceFromTuiConfig,
} from "#theme/theme";
import { initializeTreeSitterClient } from "#syntax/tree-sitter";

export interface StartReviewerTuiOptions {
	chooserFilePath?: string;
}

function selectInitialThemeName(
	themeCatalog: Awaited<ReturnType<typeof loadThemeCatalog>>,
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

export async function startReviewerTui(options: StartReviewerTuiOptions = {}) {
	const themeCatalog = await loadThemeCatalog();
	const themePreference = await readThemePreferenceFromTuiConfig();

	await Effect.runPromise(
		pipe(
			Effect.tryPromise(() => initializeTreeSitterClient()),
			Effect.catchAll((error) =>
				Effect.sync(() => {
					console.error("Failed to initialize Tree-sitter syntax parsers:", error);
				}),
			),
		),
	);
	const initialThemeName = selectInitialThemeName(themeCatalog, themePreference);

	const renderer = await createCliRenderer({ useMouse: true });
	createRoot(renderer).render(
		<App
			themeCatalog={themeCatalog}
			initialThemeName={initialThemeName}
			initialThemeMode={themePreference.mode ?? "dark"}
			{...(options.chooserFilePath
				? { chooserFilePath: options.chooserFilePath }
				: {})}
		/>,
	);
}
