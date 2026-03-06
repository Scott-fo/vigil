import { Effect, Option } from "effect";
import { useEffect } from "react";
import type { VigilDaemonConnection } from "#daemon/client.ts";
import { useDaemonSession } from "#ui/hooks/use-daemon-session.ts";
import { useDaemonWatch } from "#ui/hooks/use-daemon-watch.ts";
import type { AppKeyboardIntent, KeyboardIntentContext } from "#ui/inputs.ts";
import { useAppKeyboardInput } from "#ui/inputs.ts";
import type { UpdateFileViewState } from "#ui/state.ts";

interface AppEffectsKeyboardProps extends KeyboardIntentContext {
	readonly onIntent: (intent: AppKeyboardIntent) => void;
}

interface AppEffectsThemeProps {
	readonly filteredThemeNames: ReadonlyArray<string>;
	readonly isThemeModalOpen: boolean;
	readonly onSelectThemeInModal: (themeName: string) => void;
	readonly selectedThemeName: string;
	readonly setThemeSearchQuery: (query: string) => void;
}

interface AppEffectsProps {
	readonly daemonConnection: VigilDaemonConnection;
	readonly enabledWatch: boolean;
	readonly keyboard: AppEffectsKeyboardProps;
	readonly notifyDaemonDisconnected: (message: string) => void;
	readonly notifyDaemonReconnected: () => void;
	readonly onRefreshInstruction: Effect.Effect<void, never, never>;
	readonly theme: AppEffectsThemeProps;
	readonly updateFileView: UpdateFileViewState;
	readonly visibleFilePaths: string[];
}

export function AppEffects(props: AppEffectsProps) {
	const {
		filteredThemeNames,
		isThemeModalOpen,
		onSelectThemeInModal,
		selectedThemeName,
		setThemeSearchQuery,
	} = props.theme;

	useDaemonSession({
		daemonConnection: props.daemonConnection,
		enabled: true,
		onDisconnect: props.notifyDaemonDisconnected,
		onReconnect: props.notifyDaemonReconnected,
	});

	useDaemonWatch({
		daemonConnection: props.daemonConnection,
		repoPath: process.cwd(),
		enabled: props.enabledWatch,
		onRefreshInstruction: props.onRefreshInstruction,
	});

	useEffect(() => {
		if (!isThemeModalOpen) {
			return;
		}

		setThemeSearchQuery("");
	}, [isThemeModalOpen, setThemeSearchQuery]);

	useEffect(() => {
		props.updateFileView((current) => {
			if (props.visibleFilePaths.length === 0) {
				return Option.isNone(current.selectedPath)
					? current
					: { ...current, selectedPath: Option.none() };
			}

			if (
				Option.isSome(current.selectedPath) &&
				props.visibleFilePaths.includes(current.selectedPath.value)
			) {
				return current;
			}

			return {
				...current,
				selectedPath: Option.fromNullable(props.visibleFilePaths[0]),
			};
		});
	}, [props.updateFileView, props.visibleFilePaths]);

	useEffect(() => {
		if (
			!isThemeModalOpen ||
			filteredThemeNames.length === 0
		) {
			return;
		}

		if (filteredThemeNames.includes(selectedThemeName)) {
			return;
		}

		const firstFilteredThemeName = filteredThemeNames[0];
		if (firstFilteredThemeName) {
			onSelectThemeInModal(firstFilteredThemeName);
		}
	}, [
		filteredThemeNames,
		isThemeModalOpen,
		onSelectThemeInModal,
		selectedThemeName,
	]);

	useAppKeyboardInput({
		...props.keyboard,
		visibleFilePaths: [...props.visibleFilePaths],
	});

	return null;
}
