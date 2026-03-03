import { Match } from "effect";
import type { FileEntry } from "#tui/types";
import type { AppKeyboardIntent } from "#ui/inputs";

interface KeyboardIntentRouterActions {
	readonly destroyRenderer: () => void;
	readonly toggleSidebar: () => void;
	readonly toggleDiffViewMode: () => void;
	readonly closeCommitModal: () => void;
	readonly openCommitModal: () => void;
	readonly closeDiscardModal: () => void;
	readonly openDiscardModal: (file: FileEntry) => void;
	readonly confirmDiscardModal: () => void;
	readonly closeHelpModal: () => void;
	readonly openHelpModal: () => void;
	readonly initializeGitRepository: () => void;
	readonly openThemeModal: () => void;
	readonly openBranchCompareModal: () => void;
	readonly closeThemeModal: () => void;
	readonly closeBranchCompareModal: () => void;
	readonly confirmThemeModal: () => void;
	readonly confirmBranchCompareModal: () => void;
	readonly moveThemeSelection: (direction: 1 | -1) => void;
	readonly moveBranchSelection: (direction: 1 | -1) => void;
	readonly switchBranchField: () => void;
	readonly syncRemote: (direction: "pull" | "push") => void;
	readonly resetReviewMode: () => void;
	readonly scrollDiffHalfPage: (direction: "up" | "down") => void;
	readonly focusSidebarPane: () => void;
	readonly focusDiffPane: () => void;
	readonly openSelectedFile: (filePath: string) => void;
	readonly toggleSelectedFileStage: (file: FileEntry) => void;
	readonly selectFilePath: (path: string) => void;
}

export function routeKeyboardIntent(
	intent: AppKeyboardIntent,
	actions: KeyboardIntentRouterActions,
) {
	return Match.value(intent).pipe(
		Match.tagsExhaustive({
			DestroyRenderer: actions.destroyRenderer,
			ToggleSidebar: actions.toggleSidebar,
			ToggleDiffViewMode: actions.toggleDiffViewMode,
			CloseCommitModal: actions.closeCommitModal,
			OpenCommitModal: actions.openCommitModal,
			CloseDiscardModal: actions.closeDiscardModal,
			OpenDiscardModal: (typedIntent) => {
				actions.openDiscardModal(typedIntent.file);
			},
			ConfirmDiscardModal: actions.confirmDiscardModal,
			CloseHelpModal: actions.closeHelpModal,
			OpenHelpModal: actions.openHelpModal,
			InitGitRepository: actions.initializeGitRepository,
			OpenThemeModal: actions.openThemeModal,
			OpenBranchCompareModal: actions.openBranchCompareModal,
			CloseThemeModal: actions.closeThemeModal,
			CloseBranchCompareModal: actions.closeBranchCompareModal,
			ConfirmThemeModal: actions.confirmThemeModal,
			ConfirmBranchCompareModal: actions.confirmBranchCompareModal,
			MoveThemeSelection: (typedIntent) => {
				actions.moveThemeSelection(typedIntent.direction);
			},
			MoveBranchSelection: (typedIntent) => {
				actions.moveBranchSelection(typedIntent.direction);
			},
			SwitchBranchModalField: actions.switchBranchField,
			SyncRemote: (typedIntent) => {
				actions.syncRemote(typedIntent.direction);
			},
			ResetReviewMode: actions.resetReviewMode,
			ScrollDiffHalfPage: (typedIntent) => {
				actions.scrollDiffHalfPage(typedIntent.direction);
			},
			FocusSidebarPane: actions.focusSidebarPane,
			FocusDiffPane: actions.focusDiffPane,
			OpenSelectedFile: (typedIntent) => {
				actions.openSelectedFile(typedIntent.filePath);
			},
			ToggleSelectedFileStage: (typedIntent) => {
				actions.toggleSelectedFileStage(typedIntent.file);
			},
			SelectVisiblePath: (typedIntent) => {
				actions.selectFilePath(typedIntent.path);
			},
		}),
	);
}
