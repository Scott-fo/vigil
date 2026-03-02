import type { ScrollBoxRenderable } from "@opentui/core";
import { Effect, Match, Option, pipe } from "effect";
import {
	type Dispatch,
	type RefObject,
	type SetStateAction,
	useCallback,
} from "react";
import {
	openFileInEditor,
	renderOpenFileError,
	writeChooserSelection,
} from "#data/editor";
import {
	commitStagedChanges,
	discardFileChanges,
	initGitRepository,
	pullFromRemote,
	pushToRemote,
	type RepoActionError,
	toggleFileStage,
} from "#data/git";
import { type ThemeCatalog, type ThemeMode } from "#theme/theme";
import type { FileEntry } from "#tui/types";
import { useBranchCompareActions } from "#ui/hooks/use-branch-compare-actions";
import type { AppKeyboardIntent } from "#ui/inputs";
import { useThemeActions } from "#ui/hooks/use-theme-actions";
import type {
	BranchCompareModalState,
	CommitModalState,
	DiscardModalState,
	HelpModalState,
	ReviewMode,
	ThemeModalState,
	RemoteSyncState,
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
	readonly helpModal: HelpModalState;
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
		helpModal,
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

	const submitCommit = useCallback(
		(rawMessage: string) => {
			const result = runAction(
				commitStagedChanges(rawMessage),
				renderRepoActionError,
				{
					onSuccess: () => {
						updateCommitModal((current) =>
							current.isOpen ? { isOpen: false } : current,
						);
					},
				},
			);
			if (!result.ok) {
				updateCommitModal((current) =>
					current.isOpen
						? { ...current, error: Option.some(result.error) }
						: current,
				);
			}
		},
		[renderRepoActionError, runAction, updateCommitModal],
	);

	const openSelectedFile = useCallback(
		(filePath: string) => {
			if (Option.isSome(chooserFilePath)) {
				void Effect.runPromise(
					pipe(
						writeChooserSelection(chooserFilePath.value, filePath),
						Effect.match({
							onFailure: (error) => {
								setUiError(renderOpenFileError(error));
							},
							onSuccess: () => {
								clearUiError();
								renderer.destroy();
							},
						}),
					),
				);
				return;
			}

			renderer.suspend();
			runAction(openFileInEditor(filePath), renderOpenFileError, {
				refreshOnFailure: true,
			});
			renderer.resume();
		},
		[chooserFilePath, clearUiError, renderer, runAction, setUiError],
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

	const onCommitMessageChange = useCallback(
		(value: string) => {
			updateCommitModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				return {
					...current,
					message: value,
					error: Option.none(),
				};
			});
		},
		[updateCommitModal],
	);

	const onCommitSubmit = useCallback(
		(payload: unknown) => {
			if (typeof payload === "string") {
				submitCommit(payload);
				return;
			}
			if (!commitModal.isOpen) {
				return;
			}
			submitCommit(commitModal.message);
		},
		[commitModal, submitCommit],
	);

	const closeCommitModal = useCallback(() => {
		updateCommitModal((current) =>
			current.isOpen ? { isOpen: false } : current,
		);
	}, [updateCommitModal]);

	const closeDiscardModal = useCallback(() => {
		updateDiscardModal((current) =>
			current.isOpen ? { isOpen: false } : current,
		);
	}, [updateDiscardModal]);

	const openDiscardModal = useCallback(
		(file: FileEntry) => {
			if (reviewMode._tag !== "working-tree") {
				return;
			}
			updateDiscardModal(() => ({
				isOpen: true,
				file,
			}));
			clearUiError();
		},
		[clearUiError, reviewMode._tag, updateDiscardModal],
	);

	const confirmDiscardModal = useCallback(() => {
		if (!discardModal.isOpen) {
			return;
		}
		runAction(
			discardFileChanges(discardModal.file),
			renderRepoActionError,
			{
				onSuccess: () => {
					updateDiscardModal(() => ({ isOpen: false }));
				},
			},
		);
	}, [discardModal, renderRepoActionError, runAction, updateDiscardModal]);

	const openCommitModal = useCallback(() => {
		if (reviewMode._tag !== "working-tree") {
			return;
		}
		if (stagedFileCount === 0) {
			return;
		}
		updateCommitModal(() => ({
			isOpen: true,
			message: "",
			error: Option.none(),
		}));
		clearUiError();
	}, [clearUiError, reviewMode._tag, stagedFileCount, updateCommitModal]);

	const closeHelpModal = useCallback(() => {
		updateHelpModal((current) =>
			current.isOpen ? { isOpen: false } : current,
		);
	}, [updateHelpModal]);

	const openHelpModal = useCallback(() => {
		if (helpModal.isOpen) {
			return;
		}
		updateHelpModal(() => ({ isOpen: true }));
	}, [helpModal.isOpen, updateHelpModal]);

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

	const resetReviewMode = useCallback(() => {
		if (reviewMode._tag === "working-tree") {
			return;
		}
		updateReviewMode(() => ({ _tag: "working-tree" }));
		clearUiError();
		void refreshFiles(true);
	}, [clearUiError, refreshFiles, reviewMode._tag, updateReviewMode]);

	const syncRemote = useCallback(
		(direction: "pull" | "push") => {
			if (remoteSync._tag === "running") {
				return;
			}

			updateRemoteSync(() => ({
				_tag: "running",
				direction,
			}));
			clearUiError();

			void Effect.runPromise(
				pipe(
					direction === "push" ? pushToRemote() : pullFromRemote(),
					Effect.match({
						onFailure: (error) => {
							setUiError(renderRepoActionError(error));
						},
						onSuccess: () => {
							clearUiError();
							void refreshFiles(false);
						},
					}),
					Effect.ensuring(
						Effect.sync(() => {
							updateRemoteSync(() => ({ _tag: "idle" }));
						}),
					),
				),
			);
		},
		[
			clearUiError,
			refreshFiles,
			remoteSync,
			renderRepoActionError,
			setUiError,
			updateRemoteSync,
		],
	);

	const toggleSelectedFileStage = useCallback(
		(file: FileEntry) => {
			if (reviewMode._tag !== "working-tree") {
				return;
			}
			runAction(toggleFileStage(file), renderRepoActionError);
		},
		[renderRepoActionError, reviewMode._tag, runAction],
	);

	const initializeGitRepository = useCallback(() => {
		if (!canInitializeGitRepo) {
			return;
		}
		runAction(initGitRepository(), renderRepoActionError, {
			refreshOnFailure: true,
		});
	}, [canInitializeGitRepo, renderRepoActionError, runAction]);

	const onKeyboardIntent = useCallback(
		(intent: AppKeyboardIntent) =>
			Match.value(intent).pipe(
				Match.tagsExhaustive({
					DestroyRenderer: () => {
						renderer.destroy();
					},
					ToggleSidebar: () => {
						toggleSidebar();
					},
					ToggleDiffViewMode: () => {
						toggleDiffViewMode();
					},
					CloseCommitModal: () => {
						closeCommitModal();
					},
					OpenCommitModal: () => {
						openCommitModal();
					},
					CloseDiscardModal: () => {
						closeDiscardModal();
					},
					OpenDiscardModal: (typedIntent) => {
						openDiscardModal(typedIntent.file);
					},
					ConfirmDiscardModal: () => {
						confirmDiscardModal();
					},
					CloseHelpModal: () => {
						closeHelpModal();
					},
					OpenHelpModal: () => {
						openHelpModal();
					},
					InitGitRepository: () => {
						initializeGitRepository();
					},
					OpenThemeModal: () => {
						openThemeModal();
					},
					OpenBranchCompareModal: () => {
						openBranchCompareModal();
					},
					CloseThemeModal: () => {
						closeThemeModal();
					},
					CloseBranchCompareModal: () => {
						closeBranchCompareModal();
					},
					ConfirmThemeModal: () => {
						confirmThemeModal();
					},
					ConfirmBranchCompareModal: () => {
						confirmBranchCompareModal();
					},
					MoveThemeSelection: (typedIntent) => {
						moveThemeSelection(typedIntent.direction);
					},
					MoveBranchSelection: (typedIntent) => {
						moveBranchSelection(typedIntent.direction);
					},
					SwitchBranchModalField: () => {
						switchBranchField();
					},
					SyncRemote: (typedIntent) => {
						syncRemote(typedIntent.direction);
					},
					ResetReviewMode: () => {
						resetReviewMode();
					},
					ScrollDiffHalfPage: (typedIntent) => {
						const step = Math.max(6, Math.floor(renderer.height * 0.45));
						diffScrollRef.current?.scrollBy({
							x: 0,
							y: typedIntent.direction === "up" ? -step : step,
						});
					},
					OpenSelectedFile: (typedIntent) => {
						openSelectedFile(typedIntent.filePath);
					},
					ToggleSelectedFileStage: (typedIntent) => {
						toggleSelectedFileStage(typedIntent.file);
					},
					SelectVisiblePath: (typedIntent) => {
						selectFilePath(typedIntent.path);
					},
				}),
			),
		[
			closeCommitModal,
			closeDiscardModal,
			closeHelpModal,
			closeThemeModal,
			confirmDiscardModal,
			confirmThemeModal,
			initializeGitRepository,
			moveThemeSelection,
			moveBranchSelection,
			openCommitModal,
			openBranchCompareModal,
			openDiscardModal,
			openHelpModal,
			openThemeModal,
			openSelectedFile,
			renderer,
			diffScrollRef,
			resetReviewMode,
			selectFilePath,
			switchBranchField,
			syncRemote,
			toggleDiffViewMode,
			toggleSidebar,
			toggleSelectedFileStage,
			closeBranchCompareModal,
			confirmBranchCompareModal,
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
