import { Effect, Option } from "effect";
import { useEffect } from "react";
import type { VigilDaemonConnection } from "#daemon/client.ts";
import { useDaemonSession } from "#ui/hooks/use-daemon-session.ts";
import { useDaemonWatch } from "#ui/hooks/use-daemon-watch.ts";
import type { AppKeyboardIntent, FocusedPane } from "#ui/inputs.ts";
import { useAppKeyboardInput } from "#ui/inputs.ts";
import type { UpdateFileViewState } from "#ui/state.ts";
import type { FileEntry } from "#tui/types.ts";

interface AppEffectsProps {
	readonly activePane: FocusedPane;
	readonly canInitializeGitRepo: boolean;
	readonly canOpenBlameCommitCompare: boolean;
	readonly daemonConnection: VigilDaemonConnection;
	readonly enabledWatch: boolean;
	readonly isBlameViewOpen: boolean;
	readonly isBranchCompareModalOpen: boolean;
	readonly isCommitModalOpen: boolean;
	readonly isCommitSearchModalOpen: boolean;
	readonly isDiscardModalOpen: boolean;
	readonly isHelpModalOpen: boolean;
	readonly isReadOnlyReviewMode: boolean;
	readonly isThemeModalOpen: boolean;
	readonly notifyDaemonDisconnected: (message: string) => void;
	readonly notifyDaemonReconnected: () => void;
	readonly onIntent: (intent: AppKeyboardIntent) => void;
	readonly onRefreshInstruction: Effect.Effect<void, never, never>;
	readonly onSelectThemeInModal: (themeName: string) => void;
	readonly selectedDiffFilePath: string | null;
	readonly selectedDiffLineNumber: number | null;
	readonly selectedFile: FileEntry | null;
	readonly selectedThemeName: string;
	readonly selectedVisibleIndex: number;
	readonly setThemeSearchQuery: (query: string) => void;
	readonly stagedFileCount: number;
	readonly updateFileView: UpdateFileViewState;
	readonly visibleFilePaths: string[];
	readonly filteredThemeNames: ReadonlyArray<string>;
}

export function AppEffects(props: AppEffectsProps) {
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
		if (!props.isThemeModalOpen) {
			return;
		}

		props.setThemeSearchQuery("");
	}, [props.isThemeModalOpen, props.setThemeSearchQuery]);

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
		if (!props.isThemeModalOpen || props.filteredThemeNames.length === 0) {
			return;
		}

		if (props.filteredThemeNames.includes(props.selectedThemeName)) {
			return;
		}

		const firstFilteredThemeName = props.filteredThemeNames[0];
		if (firstFilteredThemeName) {
			props.onSelectThemeInModal(firstFilteredThemeName);
		}
	}, [
		props.filteredThemeNames,
		props.isThemeModalOpen,
		props.onSelectThemeInModal,
		props.selectedThemeName,
	]);

	useAppKeyboardInput({
		isBlameViewOpen: props.isBlameViewOpen,
		canOpenBlameCommitCompare: props.canOpenBlameCommitCompare,
		isCommitModalOpen: props.isCommitModalOpen,
		isDiscardModalOpen: props.isDiscardModalOpen,
		isCommitSearchModalOpen: props.isCommitSearchModalOpen,
		isHelpModalOpen: props.isHelpModalOpen,
		isThemeModalOpen: props.isThemeModalOpen,
		isBranchCompareModalOpen: props.isBranchCompareModalOpen,
		isReadOnlyReviewMode: props.isReadOnlyReviewMode,
		activePane: props.activePane,
		canInitializeGitRepo: props.canInitializeGitRepo,
		stagedFileCount: props.stagedFileCount,
		visibleFilePaths: [...props.visibleFilePaths],
		selectedVisibleIndex: props.selectedVisibleIndex,
		selectedFile: props.selectedFile,
		selectedDiffFilePath: props.selectedDiffFilePath,
		selectedDiffLineNumber: props.selectedDiffLineNumber,
		onIntent: props.onIntent,
	});

	return null;
}
