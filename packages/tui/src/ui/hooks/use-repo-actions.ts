import type { ScrollBoxRenderable } from "@opentui/core";
import { Option } from "effect";
import {
	type Dispatch,
	type RefObject,
	type SetStateAction,
	useCallback,
} from "react";
import type { RepoActionError } from "#data/git.ts";
import { useKeyboardIntentHandler } from "#ui/hooks/use-keyboard-intent-handler.ts";
import { useUiController } from "#ui/hooks/use-ui-controller.ts";
import type { ThemeCatalog, ThemeMode } from "#theme/theme.ts";
import { useBranchCompareActions } from "#ui/hooks/use-branch-compare-actions.ts";
import { useCommitSearchActions } from "#ui/hooks/use-commit-search-actions.ts";
import { useGitActions } from "#ui/hooks/use-git-actions.ts";
import { useNavigationActions } from "#ui/hooks/use-navigation-actions.ts";
import { useThemeActions } from "#ui/hooks/use-theme-actions.ts";
import type { FocusedPane } from "#ui/inputs.ts";
import type {
	BranchCompareModalState,
	CommitSearchModalState,
	CommitModalState,
	DiscardModalState,
	RemoteSyncState,
	ReviewMode,
	ThemeModalState,
	UpdateBranchCompareModal,
	UpdateCommitSearchModal,
	UpdateCommitModal,
	UpdateDiscardModal,
	UpdateFileViewState,
	UpdateHelpModal,
	UpdateRemoteSyncState,
	UpdateReviewMode,
	UpdateThemeModal,
	UpdateUiStatus,
} from "#ui/state.ts";
import { closeHelpModalState, openHelpModalState } from "#ui/state.ts";

interface RendererControls {
	readonly height: number;
	destroy(): void;
	suspend(): void;
	resume(): void;
}

interface UseRepoActionsOptions {
	readonly chooserFilePath: Option.Option<string>;
	readonly renderer: RendererControls;
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly diffLineCount: number;
	readonly themeName: string;
	readonly themeMode: ThemeMode;
	readonly themeCatalog: ThemeCatalog;
	readonly themeModalThemeNames: ReadonlyArray<string>;
	readonly setThemeName: Dispatch<SetStateAction<string>>;
	readonly stagedFileCount: number;
	readonly sidebarOpen: boolean;
	readonly activePane: FocusedPane;
	readonly setActivePane: Dispatch<SetStateAction<FocusedPane>>;
	readonly setSelectedDiffLineIndex: Dispatch<SetStateAction<number>>;
	readonly commitModal: CommitModalState;
	readonly discardModal: DiscardModalState;
	readonly commitSearchModal: CommitSearchModalState;
	readonly themeModal: ThemeModalState;
	readonly branchCompareModal: BranchCompareModalState;
	readonly remoteSync: RemoteSyncState;
	readonly reviewMode: ReviewMode;
	readonly canInitializeGitRepo: boolean;
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly updateCommitModal: UpdateCommitModal;
	readonly updateDiscardModal: UpdateDiscardModal;
	readonly updateCommitSearchModal: UpdateCommitSearchModal;
	readonly updateHelpModal: UpdateHelpModal;
	readonly updateThemeModal: UpdateThemeModal;
	readonly updateBranchCompareModal: UpdateBranchCompareModal;
	readonly updateRemoteSync: UpdateRemoteSyncState;
	readonly updateReviewMode: UpdateReviewMode;
	readonly closeBlameView: () => void;
	readonly openBlameCommitCompare: () => void;
	readonly scrollBlameView: (direction: "up" | "down") => void;
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly renderRepoActionError: (error: RepoActionError) => string;
}

export function useRepoActions(options: UseRepoActionsOptions) {
	const {
		chooserFilePath,
		renderer,
		diffScrollRef,
		diffLineCount,
		themeName,
		themeMode,
		themeCatalog,
		themeModalThemeNames,
		setThemeName,
		stagedFileCount,
		sidebarOpen,
		activePane,
		setActivePane,
		setSelectedDiffLineIndex,
		commitModal,
		discardModal,
		commitSearchModal,
		themeModal,
		branchCompareModal,
		remoteSync,
		reviewMode,
		canInitializeGitRepo,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		updateDiscardModal,
		updateCommitSearchModal,
		updateHelpModal,
		updateThemeModal,
		updateBranchCompareModal,
		updateRemoteSync,
		updateReviewMode,
		closeBlameView,
		openBlameCommitCompare,
		scrollBlameView,
		refreshFiles,
		renderRepoActionError,
	} = options;

	const uiController = useUiController({
		updateUiStatus,
		refreshFiles,
	});

	const closeHelpModal = useCallback(() => {
		updateHelpModal(closeHelpModalState);
	}, [updateHelpModal]);

	const openHelpModal = useCallback(() => {
		updateHelpModal(openHelpModalState);
	}, [updateHelpModal]);

	const {
		openThemeModal,
		closeThemeModal,
		confirmThemeModal,
		moveThemeSelection,
		selectThemeInModal,
	} = useThemeActions({
		themeModal,
		themeModalThemeNames,
		themeCatalog,
		themeName,
		themeMode,
		setThemeName,
		updateThemeModal,
		uiController,
	});

	const {
		openBranchCompareModal,
		closeBranchCompareModal,
		confirmBranchCompareModal,
		moveBranchSelection,
		switchBranchField,
		onBranchSourceQueryChange,
		onBranchDestinationQueryChange,
		onBranchSelectRef,
		onBranchActivateField,
	} = useBranchCompareActions({
		branchCompareModal,
		reviewMode,
		updateBranchCompareModal,
		updateReviewMode,
		renderRepoActionError,
		uiController,
	});

	const {
		openCommitSearchModal,
		closeCommitSearchModal,
		confirmCommitSearchModal,
		moveCommitSearchSelection,
		onCommitSearchQueryChange,
		onCommitSearchSelectCommit,
	} = useCommitSearchActions({
		commitSearchModal,
		reviewMode,
		updateCommitSearchModal,
		updateReviewMode,
		renderRepoActionError,
		uiController,
	});

	const {
		onCommitMessageChange,
		onCommitSubmit,
		closeCommitModal,
		openCommitModal,
		closeDiscardModal,
		openDiscardModal,
		confirmDiscardModal,
		openSelectedFile,
		openSelectedDiffLine,
		toggleSelectedFileStage,
		initializeGitRepository,
		resetReviewMode,
		syncRemote,
	} = useGitActions({
		chooserFilePath,
		renderer,
		reviewMode,
		remoteSync,
		stagedFileCount,
		canInitializeGitRepo,
		commitModal,
		discardModal,
		updateCommitModal,
		updateDiscardModal,
		updateRemoteSync,
		updateReviewMode,
		renderRepoActionError,
		uiController,
	});

	const {
		focusDiffPane,
		focusSidebarPane,
		moveDiffSelection,
		onSelectFilePath,
		onToggleDirectory,
		onToggleSidebar,
		scrollDiffHalfPage,
		toggleDiffViewMode,
	} = useNavigationActions({
		diffScrollRef,
		diffLineCount,
		rendererHeight: renderer.height,
		sidebarOpen,
		activePane,
		setActivePane,
		setSelectedDiffLineIndex,
		updateFileView,
	});

	const { onKeyboardIntent } = useKeyboardIntentHandler({
		destroyRenderer: renderer.destroy,
		toggleSidebar: onToggleSidebar,
		toggleDiffViewMode,
		closeBlameView,
		openBlameCommitCompare,
		scrollBlameView,
		closeCommitModal,
		openCommitModal,
		closeDiscardModal,
		openDiscardModal,
		confirmDiscardModal,
		closeHelpModal,
		openHelpModal,
		initializeGitRepository,
		openThemeModal,
		openBranchCompareModal,
		openCommitSearchModal,
		closeThemeModal,
		closeBranchCompareModal,
		closeCommitSearchModal,
		confirmThemeModal,
		confirmBranchCompareModal,
		confirmCommitSearchModal,
		moveThemeSelection,
		moveBranchSelection,
		moveCommitSearchSelection,
		switchBranchField,
		syncRemote,
		resetReviewMode,
		scrollDiffHalfPage,
		moveDiffSelection,
		focusSidebarPane,
		focusDiffPane,
		openSelectedFile,
		openSelectedDiffLine,
		toggleSelectedFileStage,
		selectFilePath: onSelectFilePath,
	});

	return {
		onCommitMessageChange,
		onCommitSubmit,
		onCancelDiscardModal: closeDiscardModal,
		onConfirmDiscardModal: confirmDiscardModal,
		onBranchSourceQueryChange,
		onBranchDestinationQueryChange,
		onBranchSelectRef,
		onBranchActivateField,
		onCommitSearchQueryChange,
		onCommitSearchSelectCommit,
		onKeyboardIntent,
		onToggleDirectory,
		onSelectFilePath,
		onSelectThemeInModal: selectThemeInModal,
		onToggleSidebar,
	};
}
