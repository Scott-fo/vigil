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
	pullFromRemote,
	pushToRemote,
	type RepoActionError,
	toggleFileStage,
} from "#data/git";
import { cycleThemeName, type ThemeCatalog } from "#theme/theme";
import type { FileEntry } from "#tui/types";
import type { AppKeyboardIntent } from "#ui/inputs";
import type {
	CommitModalState,
	UpdateCommitModal,
	UpdateFileViewState,
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
	readonly themeCatalog: ThemeCatalog;
	readonly setThemeName: Dispatch<SetStateAction<string>>;
	readonly stagedFileCount: number;
	readonly commitModal: CommitModalState;
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly updateCommitModal: UpdateCommitModal;
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
		themeCatalog,
		setThemeName,
		stagedFileCount,
		commitModal,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
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

	const cycleTheme = useCallback(
		(direction: 1 | -1) => {
			setThemeName((current) =>
				cycleThemeName(themeCatalog, current, direction),
			);
		},
		[setThemeName, themeCatalog],
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

	const onKeyboardIntent = useCallback(
		(intent: AppKeyboardIntent) =>
			Match.value(intent).pipe(
				Match.tag("DestroyRenderer", () => {
					renderer.destroy();
				}),
				Match.tag("CloseCommitModal", () => {
					closeCommitModal();
				}),
				Match.tag("OpenCommitModal", () => {
					openCommitModal();
				}),
				Match.tag("CycleTheme", (typedIntent) => {
					cycleTheme(typedIntent.direction);
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
			cycleTheme,
			openCommitModal,
			openSelectedFile,
			renderer,
			diffScrollRef,
			selectFilePath,
			syncRemote,
			toggleSelectedFileStage,
		],
	);

	return {
		onCommitMessageChange,
		onCommitSubmit,
		onKeyboardIntent,
		onToggleDirectory: toggleCollapsedDirectory,
		onSelectFilePath: selectFilePath,
	};
}
