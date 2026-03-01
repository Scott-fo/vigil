import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { App } from "./app";
import {
	loadThemeCatalog,
	readThemePreferenceFromTuiConfig,
} from "./theme";
import { initializeTreeSitterClient } from "./tree-sitter";

export async function startReviewerTui() {
	const themeCatalog = await loadThemeCatalog();
	const themePreference = await readThemePreferenceFromTuiConfig();

	try {
		await initializeTreeSitterClient();
	} catch (error) {
		console.error("Failed to initialize Tree-sitter syntax parsers:", error);
	}

	const initialThemeName =
		themePreference.theme && themeCatalog.themes[themePreference.theme]
			? themePreference.theme
			: themeCatalog.themes["catppuccin-macchiato"]
				? "catppuccin-macchiato"
				: themeCatalog.themes.opencode
					? "opencode"
					: (themeCatalog.order[0] ?? "opencode");

	const renderer = await createCliRenderer({ useMouse: true });
	createRoot(renderer).render(
		<App
			themeCatalog={themeCatalog}
			initialThemeName={initialThemeName}
			initialThemeMode={themePreference.mode ?? "dark"}
		/>,
	);
}
