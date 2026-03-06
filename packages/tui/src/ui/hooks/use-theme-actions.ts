import { Effect, Match, pipe } from "effect";
import { type Dispatch, type SetStateAction, useCallback } from "react";
import {
	closeThemeModalState,
	openThemeModalState,
	setThemeModalSelectionState,
	type ThemeModalState,
	type UpdateThemeModal,
} from "#ui/state.ts";
import type { UiControllerApi } from "#ui/services/ui-controller.ts";
import {
	persistThemePreferenceToTuiConfig,
	type ThemeCatalog,
	type ThemeMode,
	type ThemePreferencePersistError,
} from "#theme/theme.ts";

interface UseThemeActionsOptions {
	readonly themeModal: ThemeModalState;
	readonly themeModalThemeNames: ReadonlyArray<string>;
	readonly themeCatalog: ThemeCatalog;
	readonly themeName: string;
	readonly themeMode: ThemeMode;
	readonly setThemeName: Dispatch<SetStateAction<string>>;
	readonly updateThemeModal: UpdateThemeModal;
	readonly uiController: UiControllerApi;
}

function renderThemePreferencePersistError(
	error: ThemePreferencePersistError,
): string {
	return Match.value(error).pipe(
		Match.tag("TuiConfigParseError", () => "Invalid theme config. Fix it and try again."),
		Match.tag("TuiConfigReadError", (typedError) => typedError.message),
		Match.tag("TuiConfigWriteError", (typedError) => typedError.message),
		Match.exhaustive,
	);
}

export function useThemeActions(options: UseThemeActionsOptions) {
	const {
		themeModal,
		themeModalThemeNames,
		themeCatalog,
		themeName,
		themeMode,
		setThemeName,
		updateThemeModal,
		uiController,
	} = options;

	const openThemeModal = useCallback(() => {
		if (themeModal.isOpen) {
			return;
		}
		updateThemeModal(() => openThemeModalState(themeName));
	}, [themeModal.isOpen, themeName, updateThemeModal]);

	const closeThemeModal = useCallback(() => {
		if (!themeModal.isOpen) {
			return;
		}
		setThemeName(themeModal.initialThemeName);
		updateThemeModal(closeThemeModalState);
	}, [setThemeName, themeModal, updateThemeModal]);

	const confirmThemeModal = useCallback(() => {
		if (!themeModal.isOpen) {
			return;
		}
		const nextThemeName = themeModal.selectedThemeName;
		setThemeName(nextThemeName);
		updateThemeModal(closeThemeModalState);
		void Effect.runPromise(
			pipe(
				persistThemePreferenceToTuiConfig({
					theme: nextThemeName,
					mode: themeMode,
				}),
				Effect.match({
					onFailure: (error) =>
						uiController.setError(renderThemePreferencePersistError(error)),
					onSuccess: () => uiController.clearError(),
				}),
				Effect.flatten,
			),
		);
	}, [
		setThemeName,
		themeModal,
		themeMode,
		uiController,
		updateThemeModal,
	]);

	const moveThemeSelection = useCallback(
		(direction: 1 | -1) => {
			if (!themeModal.isOpen) {
				return;
			}
			if (themeModalThemeNames.length === 0) {
				return;
			}
			const currentIndex = themeModalThemeNames.indexOf(
				themeModal.selectedThemeName,
			);
			const baseIndex = currentIndex === -1 ? 0 : currentIndex;
			const nextIndex =
				(baseIndex + direction + themeModalThemeNames.length) %
				themeModalThemeNames.length;
			const nextThemeName =
				themeModalThemeNames[nextIndex] ?? themeModal.selectedThemeName;
			if (nextThemeName === themeModal.selectedThemeName) {
				return;
			}
			setThemeName(nextThemeName);
			updateThemeModal((current) =>
				setThemeModalSelectionState(current, nextThemeName),
			);
		},
		[setThemeName, themeModal, themeModalThemeNames, updateThemeModal],
	);

	const selectThemeInModal = useCallback(
		(nextThemeName: string) => {
			if (!themeModal.isOpen) {
				return;
			}
			if (!themeCatalog.themes[nextThemeName]) {
				return;
			}
			setThemeName(nextThemeName);
			updateThemeModal((current) =>
				setThemeModalSelectionState(current, nextThemeName),
			);
		},
		[setThemeName, themeCatalog.themes, themeModal.isOpen, updateThemeModal],
	);

	return {
		openThemeModal,
		closeThemeModal,
		confirmThemeModal,
		moveThemeSelection,
		selectThemeInModal,
	};
}
