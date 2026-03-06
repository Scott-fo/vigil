import { useState } from "react";
import type { ThemeCatalog, ThemeMode } from "#theme/theme.ts";
import type { ThemeModalState } from "#ui/state.ts";
import { useThemeView } from "#ui/hooks/use-theme-view.ts";

interface UseThemeStateOptions {
	readonly initialThemeMode: ThemeMode;
	readonly initialThemeName: string;
	readonly themeCatalog: ThemeCatalog;
	readonly themeModal: ThemeModalState;
}

export function useThemeState(options: UseThemeStateOptions) {
	const [themeName, setThemeName] = useState(options.initialThemeName);
	const [themeSearchQuery, setThemeSearchQuery] = useState("");
	const [themeMode] = useState<ThemeMode>(options.initialThemeMode);

	const themeView = useThemeView({
		themeCatalog: options.themeCatalog,
		themeModal: options.themeModal,
		themeMode,
		themeName,
		themeSearchQuery,
	});

	return {
		...themeView,
		themeMode,
		themeName,
		themeSearchQuery,
		setThemeName,
		setThemeSearchQuery,
	};
}
