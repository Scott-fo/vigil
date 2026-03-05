import type { ScrollBoxRenderable } from "@opentui/core";
import { Effect, Option, pipe } from "effect";
import {
	type Dispatch,
	type RefObject,
	type SetStateAction,
	useCallback,
} from "react";
import type { RepoActionError } from "#data/git.ts";
import type { ThemeCatalog, ThemeMode } from "#theme/theme.ts";
import { routeKeyboardIntent } from "#ui/hooks/keyboard-intent-router.ts";
import { useBranchCompareActions } from "#ui/hooks/use-branch-compare-actions.ts";
import { useCommitSearchActions } from "#ui/hooks/use-commit-search-actions.ts";
import { useGitActions } from "#ui/hooks/use-git-actions.ts";
import { useThemeActions } from "#ui/hooks/use-theme-actions.ts";
import type { AppKeyboardIntent, FocusedPane } from "#ui/inputs.ts";
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

interface RunActionOptions {
	readonly refreshOnSuccess?: boolean;
	readonly refreshOnFailure?: boolean;
	readonly onSuccess?: () => void;
}

type RunActionResult =
	| { readonly ok: true }
	| { readonly ok: false; readonly error: string };

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

	const clearUiError = useCallback(() => {
		updateUiStatus((current) =>
			Option.isNone(current.error)
				? current
				: { ...current, error: Option.none() },
		);
	}, [updateUiStatus]);

	const setUiError = useCallback(
		(error: string) => {
			updateUiStatus((current) =>
				Option.isSome(current.error) && current.error.value === error
					? current
					: { ...current, error: Option.some(error) },
			);
		},
		[updateUiStatus],
	);

	const runAction = useCallback(
		<E>(
			effect: Effect.Effect<void, E>,
			renderError: (error: E) => string,
			actionOptions: RunActionOptions = {},
		): RunActionResult => {
			const refreshOnSuccess = actionOptions.refreshOnSuccess ?? true;
			const refreshOnFailure = actionOptions.refreshOnFailure ?? false;
			const result = Effect.runSync(
				pipe(
					effect,
					Effect.match({
						onFailure: (error) => ({
							ok: false as const,
							error: renderError(error),
						}),
						onSuccess: () => ({ ok: true as const }),
					}),
				),
			);

			if (!result.ok) {
				setUiError(result.error);
				if (refreshOnFailure) {
					void refreshFiles(false);
				}
				return { ok: false, error: result.error };
			}

			actionOptions.onSuccess?.();
			clearUiError();
			if (refreshOnSuccess) {
				void refreshFiles(false);
			}
			return { ok: true };
		},
		[clearUiError, refreshFiles, setUiError],
	);

	const toggleCollapsedDirectory = useCallback(
		(path: string) => {
			updateFileView((current) => {
				const next = new Set(current.collapsedDirectories);
				if (next.has(path)) {
					next.delete(path);
				} else {
					next.add(path);
				}
				return { ...current, collapsedDirectories: next };
			});
		},
		[updateFileView],
	);

	const toggleSidebar = useCallback(() => {
		if (sidebarOpen && activePane === "sidebar") {
			setActivePane("diff");
		}

		updateFileView((current) => ({
			...current,
			sidebarOpen: !current.sidebarOpen,
		}));
	}, [activePane, setActivePane, sidebarOpen, updateFileView]);

	const focusSidebarPane = useCallback(() => {
		setActivePane("sidebar");
		updateFileView((current) =>
			current.sidebarOpen ? current : { ...current, sidebarOpen: true },
		);
	}, [setActivePane, updateFileView]);

	const focusDiffPane = useCallback(() => {
		setActivePane("diff");
	}, [setActivePane]);

	const toggleDiffViewMode = useCallback(() => {
		updateFileView((current) => ({
			...current,
			diffViewMode: current.diffViewMode === "split" ? "unified" : "split",
		}));
	}, [updateFileView]);

	const selectFilePath = useCallback(
		(path: string) => {
			updateFileView((current) => ({
				...current,
				selectedPath: Option.some(path),
			}));
		},
		[updateFileView],
	);

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
		clearUiError,
		setUiError,
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
		clearUiError,
		refreshFiles,
		renderRepoActionError,
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
		clearUiError,
		refreshFiles,
		renderRepoActionError,
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
		refreshFiles,
		clearUiError,
		setUiError,
		renderRepoActionError,
		runAction,
	});

	const scrollDiffHalfPage = useCallback(
		(direction: "up" | "down") => {
			const diffScroll = diffScrollRef.current;
			if (!diffScroll) {
				return;
			}

			const step = Math.max(6, Math.floor(renderer.height * 0.45));
			diffScroll.scrollBy({
				x: 0,
				y: direction === "up" ? -step : step,
			});

			if (activePane !== "diff" || diffLineCount <= 0) {
				return;
			}

			const topVisibleLine = Math.max(0, Math.floor(diffScroll.scrollTop));
			setSelectedDiffLineIndex(Math.min(topVisibleLine, diffLineCount - 1));
		},
		[
			activePane,
			diffLineCount,
			diffScrollRef,
			renderer.height,
			setSelectedDiffLineIndex,
		],
	);

	const moveDiffSelection = useCallback(
		(direction: 1 | -1) => {
			if (activePane !== "diff" || diffLineCount <= 0) {
				return;
			}

			setSelectedDiffLineIndex((current) =>
				Math.max(0, Math.min(current + direction, diffLineCount - 1)),
			);
		},
		[activePane, diffLineCount, setSelectedDiffLineIndex],
	);

	const onKeyboardIntent = useCallback(
		(intent: AppKeyboardIntent) =>
			routeKeyboardIntent(intent, {
				destroyRenderer: () => {
					renderer.destroy();
				},
				toggleSidebar,
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
				selectFilePath,
			}),
		[
			closeBranchCompareModal,
			closeBlameView,
			closeCommitSearchModal,
			closeCommitModal,
			closeDiscardModal,
			closeHelpModal,
			closeThemeModal,
			confirmBranchCompareModal,
			confirmCommitSearchModal,
			confirmDiscardModal,
			confirmThemeModal,
			initializeGitRepository,
			moveBranchSelection,
			moveCommitSearchSelection,
			moveThemeSelection,
			openBranchCompareModal,
			openBlameCommitCompare,
			openCommitSearchModal,
			openCommitModal,
			openDiscardModal,
			openHelpModal,
			openSelectedFile,
			openThemeModal,
			renderer.destroy,
			resetReviewMode,
			scrollDiffHalfPage,
			scrollBlameView,
			moveDiffSelection,
			focusSidebarPane,
			focusDiffPane,
			selectFilePath,
			switchBranchField,
			syncRemote,
			toggleDiffViewMode,
			openSelectedDiffLine,
			toggleSelectedFileStage,
			toggleSidebar,
		],
	);

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
		onToggleDirectory: toggleCollapsedDirectory,
		onSelectFilePath: selectFilePath,
		onSelectThemeInModal: selectThemeInModal,
		onToggleSidebar: toggleSidebar,
	};
}
