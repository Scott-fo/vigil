import { Match } from "effect";
import type { FileEntry } from "#tui/types.ts";
import type { AppKeyboardIntent } from "#ui/inputs.ts";

interface KeyboardIntentRouterActions {
	readonly destroyRenderer: () => void;
	readonly toggleSidebar: () => void;
	readonly toggleDiffViewMode: () => void;
	readonly closeBlameView: () => void;
	readonly openBlameCommitCompare: () => void;
	readonly scrollBlameView: (direction: "up" | "down") => void;
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
	readonly openCommitSearchModal: () => void;
	readonly closeThemeModal: () => void;
	readonly closeBranchCompareModal: () => void;
	readonly closeCommitSearchModal: () => void;
	readonly confirmThemeModal: () => void;
	readonly confirmBranchCompareModal: () => void;
	readonly confirmCommitSearchModal: () => void;
	readonly moveThemeSelection: (direction: 1 | -1) => void;
	readonly moveBranchSelection: (direction: 1 | -1) => void;
	readonly moveCommitSearchSelection: (direction: 1 | -1) => void;
	readonly switchBranchField: () => void;
	readonly syncRemote: (direction: "pull" | "push") => void;
	readonly resetReviewMode: () => void;
	readonly scrollDiffHalfPage: (direction: "up" | "down") => void;
	readonly moveDiffSelection: (direction: 1 | -1) => void;
	readonly focusSidebarPane: () => void;
	readonly focusDiffPane: () => void;
	readonly openSelectedFile: (filePath: string) => void;
	readonly openSelectedDiffLine: (filePath: string, lineNumber: number) => void;
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
			CloseBlameView: actions.closeBlameView,
			OpenBlameCommitCompare: actions.openBlameCommitCompare,
			ScrollBlameView: (typedIntent) => {
				actions.scrollBlameView(typedIntent.direction);
			},
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
			OpenCommitSearchModal: actions.openCommitSearchModal,
			CloseThemeModal: actions.closeThemeModal,
			CloseBranchCompareModal: actions.closeBranchCompareModal,
			CloseCommitSearchModal: actions.closeCommitSearchModal,
			ConfirmThemeModal: actions.confirmThemeModal,
			ConfirmBranchCompareModal: actions.confirmBranchCompareModal,
			ConfirmCommitSearchModal: actions.confirmCommitSearchModal,
			MoveThemeSelection: (typedIntent) => {
				actions.moveThemeSelection(typedIntent.direction);
			},
			MoveBranchSelection: (typedIntent) => {
				actions.moveBranchSelection(typedIntent.direction);
			},
			MoveCommitSearchSelection: (typedIntent) => {
				actions.moveCommitSearchSelection(typedIntent.direction);
			},
			SwitchBranchModalField: actions.switchBranchField,
			SyncRemote: (typedIntent) => {
				actions.syncRemote(typedIntent.direction);
			},
			ResetReviewMode: actions.resetReviewMode,
			ScrollDiffHalfPage: (typedIntent) => {
				actions.scrollDiffHalfPage(typedIntent.direction);
			},
			MoveDiffLineSelection: (typedIntent) => {
				actions.moveDiffSelection(typedIntent.direction);
			},
			FocusSidebarPane: actions.focusSidebarPane,
			FocusDiffPane: actions.focusDiffPane,
			OpenSelectedFile: (typedIntent) => {
				actions.openSelectedFile(typedIntent.filePath);
			},
			OpenSelectedDiffLine: (typedIntent) => {
				actions.openSelectedDiffLine(
					typedIntent.filePath,
					typedIntent.lineNumber,
				);
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
