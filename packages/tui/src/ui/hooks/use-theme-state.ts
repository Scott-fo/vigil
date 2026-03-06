import {
	type Dispatch,
	type SetStateAction,
	useCallback,
	useEffect,
	useState,
} from "react";
import type { ThemeCatalog, ThemeMode } from "#theme/theme.ts";
import type { ThemeModalState } from "#ui/state.ts";
import { useThemeView } from "#ui/hooks/use-theme-view.ts";

interface UseThemeStateOptions {
	readonly themeMode: ThemeMode;
	readonly themeName: string;
	readonly themeCatalog: ThemeCatalog;
	readonly themeModal: ThemeModalState;
}

export function useThemeState(options: UseThemeStateOptions) {
	const [themeName, setThemeNameState] = useState(options.themeName);
	const [themeSearchQuery, setThemeSearchQuery] = useState("");
	const [themeMode, setThemeMode] = useState<ThemeMode>(options.themeMode);
	const [hasLocalThemeOverride, setHasLocalThemeOverride] = useState(false);

	const setThemeName: Dispatch<SetStateAction<string>> = useCallback(
		(update) => {
			setHasLocalThemeOverride(true);
			setThemeNameState(update);
		},
		[],
	);

	useEffect(() => {
		setThemeMode(options.themeMode);
	}, [options.themeMode]);

	useEffect(() => {
		setThemeNameState((currentThemeName) => {
			if (!hasLocalThemeOverride) {
				return options.themeName;
			}

			return options.themeCatalog.themes[currentThemeName]
				? currentThemeName
				: options.themeName;
		});
	}, [hasLocalThemeOverride, options.themeCatalog.themes, options.themeName]);

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
