import { RGBA } from "@opentui/core";
import { useMemo } from "react";
import {
	resolveThemeBundle,
	type ThemeCatalog,
	type ThemeMode,
} from "#theme/theme.ts";
import type { ThemeModalState } from "#ui/state.ts";

interface UseThemeViewOptions {
	readonly themeCatalog: ThemeCatalog;
	readonly themeModal: ThemeModalState;
	readonly themeMode: ThemeMode;
	readonly themeName: string;
	readonly themeSearchQuery: string;
}

export function useThemeView(options: UseThemeViewOptions) {
	const { themeCatalog, themeModal, themeMode, themeName, themeSearchQuery } =
		options;

	const themeBundle = useMemo(
		() => resolveThemeBundle(themeCatalog, themeName, themeMode),
		[themeCatalog, themeName, themeMode],
	);

	const theme = themeBundle.theme;

	const modalBackdropColor = RGBA.fromValues(
		theme.background.r,
		theme.background.g,
		theme.background.b,
		0.55,
	);

	const filteredThemeNames = useMemo(() => {
		const query = themeSearchQuery.trim().toLowerCase();
		if (query.length === 0) {
			return themeCatalog.order;
		}

		return themeCatalog.order.filter((themeCatalogName) =>
			themeCatalogName.toLowerCase().includes(query),
		);
	}, [themeCatalog.order, themeSearchQuery]);

	const selectedThemeName = themeModal.isOpen
		? themeModal.selectedThemeName
		: themeName;

	return {
		filteredThemeNames,
		modalBackdropColor,
		selectedThemeName,
		theme,
		themeBundle,
	};
}
