import type { ScrollBoxRenderable } from "@opentui/core";
import { Effect, Option, pipe } from "effect";
import {
	type Dispatch,
	type RefObject,
	type SetStateAction,
	useCallback,
} from "react";
import type { RepoActionError } from "#data/git";
import { type ThemeCatalog, type ThemeMode } from "#theme/theme";
import { useBranchCompareActions } from "#ui/hooks/use-branch-compare-actions";
import { useGitActions } from "#ui/hooks/use-git-actions";
import { routeKeyboardIntent } from "#ui/hooks/keyboard-intent-router";
import { useThemeActions } from "#ui/hooks/use-theme-actions";
import type { AppKeyboardIntent } from "#ui/inputs";
import type {
	BranchCompareModalState,
	CommitModalState,
	DiscardModalState,
	RemoteSyncState,
	ReviewMode,
	ThemeModalState,
	UpdateBranchCompareModal,
	UpdateCommitModal,
	UpdateDiscardModal,
	UpdateFileViewState,
	UpdateHelpModal,
	UpdateReviewMode,
	UpdateRemoteSyncState,
	UpdateThemeModal,
	UpdateUiStatus,
} from "#ui/state";
import { closeHelpModalState, openHelpModalState } from "#ui/state";

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
	readonly themeName: string;
	readonly themeMode: ThemeMode;
	readonly themeCatalog: ThemeCatalog;
	readonly themeModalThemeNames: ReadonlyArray<string>;
	readonly setThemeName: Dispatch<SetStateAction<string>>;
	readonly stagedFileCount: number;
	readonly commitModal: CommitModalState;
	readonly discardModal: DiscardModalState;
	readonly themeModal: ThemeModalState;
	readonly branchCompareModal: BranchCompareModalState;
	readonly remoteSync: RemoteSyncState;
	readonly reviewMode: ReviewMode;
	readonly canInitializeGitRepo: boolean;
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly updateCommitModal: UpdateCommitModal;
	readonly updateDiscardModal: UpdateDiscardModal;
	readonly updateHelpModal: UpdateHelpModal;
	readonly updateThemeModal: UpdateThemeModal;
	readonly updateBranchCompareModal: UpdateBranchCompareModal;
	readonly updateRemoteSync: UpdateRemoteSyncState;
	readonly updateReviewMode: UpdateReviewMode;
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
		themeName,
		themeMode,
		themeCatalog,
		themeModalThemeNames,
		setThemeName,
		stagedFileCount,
		commitModal,
		discardModal,
		themeModal,
		branchCompareModal,
		remoteSync,
		reviewMode,
		canInitializeGitRepo,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		updateDiscardModal,
		updateHelpModal,
		updateThemeModal,
		updateBranchCompareModal,
		updateRemoteSync,
		updateReviewMode,
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
		<E,>(
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
		updateFileView((current) => ({
			...current,
			sidebarOpen: !current.sidebarOpen,
		}));
	}, [updateFileView]);

	const toggleDiffViewMode = useCallback(() => {
		updateFileView((current) => ({
			...current,
			diffViewMode:
				current.diffViewMode === "split" ? "unified" : "split",
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
		onCommitMessageChange,
		onCommitSubmit,
		closeCommitModal,
		openCommitModal,
		closeDiscardModal,
		openDiscardModal,
		confirmDiscardModal,
		openSelectedFile,
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
			const step = Math.max(6, Math.floor(renderer.height * 0.45));
			diffScrollRef.current?.scrollBy({
				x: 0,
				y: direction === "up" ? -step : step,
			});
		},
		[diffScrollRef, renderer.height],
	);

	const onKeyboardIntent = useCallback(
		(intent: AppKeyboardIntent) =>
			routeKeyboardIntent(intent, {
				destroyRenderer: () => {
					renderer.destroy();
				},
				toggleSidebar,
				toggleDiffViewMode,
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
				closeThemeModal,
				closeBranchCompareModal,
				confirmThemeModal,
				confirmBranchCompareModal,
				moveThemeSelection,
				moveBranchSelection,
				switchBranchField,
				syncRemote,
				resetReviewMode,
				scrollDiffHalfPage,
				openSelectedFile,
				toggleSelectedFileStage,
				selectFilePath,
			}),
		[
			closeBranchCompareModal,
			closeCommitModal,
			closeDiscardModal,
			closeHelpModal,
			closeThemeModal,
			confirmBranchCompareModal,
			confirmDiscardModal,
			confirmThemeModal,
			initializeGitRepository,
			moveBranchSelection,
			moveThemeSelection,
			openBranchCompareModal,
			openCommitModal,
			openDiscardModal,
			openHelpModal,
			openSelectedFile,
			openThemeModal,
			renderer.destroy,
			resetReviewMode,
			scrollDiffHalfPage,
			selectFilePath,
			switchBranchField,
			syncRemote,
			toggleDiffViewMode,
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
		onKeyboardIntent,
		onToggleDirectory: toggleCollapsedDirectory,
		onSelectFilePath: selectFilePath,
		onSelectThemeInModal: selectThemeInModal,
		onToggleSidebar: toggleSidebar,
	};
}
