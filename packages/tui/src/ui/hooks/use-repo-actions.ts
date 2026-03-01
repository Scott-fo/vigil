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
	initGitRepository,
	pullFromRemote,
	pushToRemote,
	type RepoActionError,
	toggleFileStage,
} from "#data/git";
import type { ThemeCatalog } from "#theme/theme";
import type { FileEntry } from "#tui/types";
import type { AppKeyboardIntent } from "#ui/inputs";
import type {
	CommitModalState,
	HelpModalState,
	ThemeModalState,
	UpdateCommitModal,
	UpdateFileViewState,
	UpdateHelpModal,
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
	readonly themeCatalog: ThemeCatalog;
	readonly themeModalThemeNames: ReadonlyArray<string>;
	readonly setThemeName: Dispatch<SetStateAction<string>>;
	readonly stagedFileCount: number;
	readonly commitModal: CommitModalState;
	readonly helpModal: HelpModalState;
	readonly themeModal: ThemeModalState;
	readonly canInitializeGitRepo: boolean;
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly updateCommitModal: UpdateCommitModal;
	readonly updateHelpModal: UpdateHelpModal;
	readonly updateThemeModal: UpdateThemeModal;
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
		themeCatalog,
		themeModalThemeNames,
		setThemeName,
		stagedFileCount,
		commitModal,
		helpModal,
		themeModal,
		canInitializeGitRepo,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		updateHelpModal,
		updateThemeModal,
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
				const wrote = runAction(
					writeChooserSelection(chooserFilePath.value, filePath),
					renderOpenFileError,
					{
						refreshOnSuccess: false,
						refreshOnFailure: false,
						onSuccess: () => {
							renderer.destroy();
						},
					},
				);
				if (!wrote.ok) {
					return;
				}
				return;
			}

			renderer.suspend();
			runAction(openFileInEditor(filePath), renderOpenFileError, {
				refreshOnFailure: true,
			});
			renderer.resume();
		},
		[chooserFilePath, renderer, runAction],
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

	const openCommitModal = useCallback(() => {
		if (stagedFileCount === 0) {
			return;
		}
		updateCommitModal(() => ({
			isOpen: true,
			message: "",
			error: Option.none(),
		}));
		clearUiError();
	}, [clearUiError, stagedFileCount, updateCommitModal]);

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

	const openThemeModal = useCallback(() => {
		if (themeModal.isOpen) {
			return;
		}
		updateThemeModal(() => ({
			isOpen: true,
			initialThemeName: themeName,
			selectedThemeName: themeName,
		}));
	}, [themeModal.isOpen, themeName, updateThemeModal]);

	const closeThemeModal = useCallback(() => {
		if (!themeModal.isOpen) {
			return;
		}
		setThemeName(themeModal.initialThemeName);
		updateThemeModal(() => ({ isOpen: false }));
	}, [setThemeName, themeModal, updateThemeModal]);

	const confirmThemeModal = useCallback(() => {
		if (!themeModal.isOpen) {
			return;
		}
		updateThemeModal(() => ({ isOpen: false }));
	}, [themeModal.isOpen, updateThemeModal]);

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
				current.isOpen
					? { ...current, selectedThemeName: nextThemeName }
					: current,
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
				current.isOpen
					? { ...current, selectedThemeName: nextThemeName }
					: current,
			);
		},
		[setThemeName, themeCatalog.themes, themeModal.isOpen, updateThemeModal],
	);

	const syncRemote = useCallback(
		(direction: "pull" | "push") => {
			runAction(
				direction === "push" ? pushToRemote() : pullFromRemote(),
				renderRepoActionError,
			);
		},
		[renderRepoActionError, runAction],
	);

	const toggleSelectedFileStage = useCallback(
		(file: FileEntry) => {
			runAction(toggleFileStage(file), renderRepoActionError);
		},
		[renderRepoActionError, runAction],
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
				Match.tag("DestroyRenderer", () => {
					renderer.destroy();
				}),
				Match.tag("ToggleSidebar", () => {
					toggleSidebar();
				}),
				Match.tag("ToggleDiffViewMode", () => {
					toggleDiffViewMode();
				}),
				Match.tag("CloseCommitModal", () => {
					closeCommitModal();
				}),
				Match.tag("OpenCommitModal", () => {
					openCommitModal();
				}),
				Match.tag("CloseHelpModal", () => {
					closeHelpModal();
				}),
				Match.tag("OpenHelpModal", () => {
					openHelpModal();
				}),
				Match.tag("InitGitRepository", () => {
					initializeGitRepository();
				}),
				Match.tag("OpenThemeModal", () => {
					openThemeModal();
				}),
				Match.tag("CloseThemeModal", () => {
					closeThemeModal();
				}),
				Match.tag("ConfirmThemeModal", () => {
					confirmThemeModal();
				}),
				Match.tag("MoveThemeSelection", (typedIntent) => {
					moveThemeSelection(typedIntent.direction);
				}),
				Match.tag("SyncRemote", (typedIntent) => {
					syncRemote(typedIntent.direction);
				}),
				Match.tag("ScrollDiffHalfPage", (typedIntent) => {
					const step = Math.max(6, Math.floor(renderer.height * 0.45));
					diffScrollRef.current?.scrollBy({
						x: 0,
						y: typedIntent.direction === "up" ? -step : step,
					});
				}),
				Match.tag("OpenSelectedFile", (typedIntent) => {
					openSelectedFile(typedIntent.filePath);
				}),
				Match.tag("ToggleSelectedFileStage", (typedIntent) => {
					toggleSelectedFileStage(typedIntent.file);
				}),
				Match.tag("SelectVisiblePath", (typedIntent) => {
					selectFilePath(typedIntent.path);
				}),
				Match.exhaustive,
			),
		[
			closeCommitModal,
			closeHelpModal,
			closeThemeModal,
			confirmThemeModal,
			helpModal.isOpen,
			initializeGitRepository,
			moveThemeSelection,
			openCommitModal,
			openHelpModal,
			openThemeModal,
			openSelectedFile,
			renderer,
			diffScrollRef,
			selectFilePath,
			syncRemote,
			toggleDiffViewMode,
			toggleSidebar,
			toggleSelectedFileStage,
		],
	);

	return {
		onCommitMessageChange,
		onCommitSubmit,
		onKeyboardIntent,
		onToggleDirectory: toggleCollapsedDirectory,
		onSelectFilePath: selectFilePath,
		onSelectThemeInModal: selectThemeInModal,
		onToggleSidebar: toggleSidebar,
	};
}
