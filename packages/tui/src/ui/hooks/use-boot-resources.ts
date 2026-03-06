import { Effect } from "effect";
import { useEffect, useState } from "react";
import { initializeTreeSitterClient } from "#syntax/tree-sitter.ts";
import {
	getFallbackThemeCatalog,
	loadThemeCatalog,
	readThemePreferenceFromTuiConfig,
	selectStartupThemeName,
	type ThemeCatalog,
	type ThemeMode,
} from "#theme/theme.ts";

type BootResources = {
	readonly themeCatalog: ThemeCatalog;
	readonly themeMode: ThemeMode;
	readonly themeName: string;
};

type BootState = BootResources & {
	readonly hasExplicitThemePreference: boolean;
};

const DEFAULT_THEME_MODE: ThemeMode = "dark";
const fallbackThemeCatalog = getFallbackThemeCatalog();
const initialBootState: BootState = {
	themeCatalog: fallbackThemeCatalog,
	themeMode: DEFAULT_THEME_MODE,
	themeName: selectStartupThemeName(fallbackThemeCatalog),
	hasExplicitThemePreference: false,
};

function renderBootError(message: string, cause: unknown): string {
	const details = cause instanceof Error ? cause.message : String(cause);
	return `${message} ${details}`;
}

export function useBootResources(): BootResources {
	const [bootState, setBootState] = useState(initialBootState);

	useEffect(() => {
		let isActive = true;

		void loadThemeCatalog()
			.then((themeCatalog) => {
				if (!isActive) {
					return;
				}

				setBootState((current) => ({
					...current,
					themeCatalog,
					themeName:
						current.hasExplicitThemePreference ||
						current.themeName !== selectStartupThemeName(current.themeCatalog)
							? current.themeName
							: selectStartupThemeName(themeCatalog),
				}));
			})
			.catch((cause) => {
				console.error(renderBootError("Failed to load theme catalog.", cause));
			});

		void readThemePreferenceFromTuiConfig()
			.then((themePreference) => {
				if (!isActive) {
					return;
				}

				setBootState((current) => ({
					...current,
					themeName: themePreference.theme ?? current.themeName,
					themeMode: themePreference.mode ?? current.themeMode,
					hasExplicitThemePreference: themePreference.theme !== undefined,
				}));
			})
			.catch((cause) => {
				console.error(
					renderBootError("Failed to read theme preference configuration.", cause),
				);
			});

		void Effect.runPromise(
			initializeTreeSitterClient().pipe(
				Effect.catchTag("TreeSitterInitializeError", (typedError) =>
					Effect.sync(() => {
						console.error(
							renderBootError(
								"Failed to initialize Tree-sitter syntax parsers.",
								typedError.cause ?? typedError.message,
							),
						);
					}),
				),
				Effect.asVoid,
			),
		);

		return () => {
			isActive = false;
		};
	}, []);

	return {
		themeCatalog: bootState.themeCatalog,
		themeMode: bootState.themeMode,
		themeName: bootState.themeName,
	};
}
