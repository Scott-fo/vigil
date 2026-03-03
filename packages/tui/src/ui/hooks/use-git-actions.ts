import { Effect, Option, pipe } from "effect";
import { useCallback } from "react";
import {
	openFileInEditorAtLine,
	openFileInEditor,
	renderOpenFileError,
	writeChooserSelection,
} from "#data/editor.ts";
import {
	commitStagedChanges,
	discardFileChanges,
	initGitRepository,
	pullFromRemote,
	pushToRemote,
	type RepoActionError,
	toggleFileStage,
} from "#data/git.ts";
import type { FileEntry } from "#tui/types.ts";
import type {
	CommitModalState,
	DiscardModalState,
	RemoteSyncState,
	ReviewMode,
	UpdateCommitModal,
	UpdateDiscardModal,
	UpdateRemoteSyncState,
	UpdateReviewMode,
} from "#ui/state.ts";
import {
	closeCommitModalState,
	closeDiscardModalState,
	isWorkingTreeReviewMode,
	openCommitModalState,
	openDiscardModalState,
	setCommitModalErrorState,
	setCommitModalMessageState,
} from "#ui/state.ts";

interface RendererControls {
	readonly height: number;
	destroy(): void;
	suspend(): void;
	resume(): void;
}

interface RunActionOptions {
	readonly refreshOnSuccess?: boolean;
	readonly refreshOnFailure?: boolean;
	readonly onSuccess?: () => void;
}

type RunActionResult =
	| { readonly ok: true }
	| { readonly ok: false; readonly error: string };

interface UseGitActionsOptions {
	readonly chooserFilePath: Option.Option<string>;
	readonly renderer: RendererControls;
	readonly reviewMode: ReviewMode;
	readonly remoteSync: RemoteSyncState;
	readonly stagedFileCount: number;
	readonly canInitializeGitRepo: boolean;
	readonly commitModal: CommitModalState;
	readonly discardModal: DiscardModalState;
	readonly updateCommitModal: UpdateCommitModal;
	readonly updateDiscardModal: UpdateDiscardModal;
	readonly updateRemoteSync: UpdateRemoteSyncState;
	readonly updateReviewMode: UpdateReviewMode;
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly clearUiError: () => void;
	readonly setUiError: (error: string) => void;
	readonly renderRepoActionError: (error: RepoActionError) => string;
	readonly runAction: <E>(
		effect: Effect.Effect<void, E>,
		renderError: (error: E) => string,
		actionOptions?: RunActionOptions,
	) => RunActionResult;
}

export function useGitActions(options: UseGitActionsOptions) {
	const {
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
	} = options;

	const submitCommit = useCallback(
		(rawMessage: string) => {
			const result = runAction(
				commitStagedChanges(rawMessage),
				renderRepoActionError,
				{
					onSuccess: () => {
						updateCommitModal(closeCommitModalState);
					},
				},
			);
			if (!result.ok) {
				updateCommitModal((current) =>
					setCommitModalErrorState(current, result.error),
				);
			}
		},
		[renderRepoActionError, runAction, updateCommitModal],
	);

	const onCommitMessageChange = useCallback(
		(value: string) => {
			updateCommitModal((current) =>
				setCommitModalMessageState(current, value),
			);
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
		updateCommitModal(closeCommitModalState);
	}, [updateCommitModal]);

	const openCommitModal = useCallback(() => {
		if (!isWorkingTreeReviewMode(reviewMode)) {
			return;
		}
		if (stagedFileCount === 0) {
			return;
		}
		updateCommitModal(openCommitModalState);
		clearUiError();
	}, [clearUiError, reviewMode, stagedFileCount, updateCommitModal]);

	const closeDiscardModal = useCallback(() => {
		updateDiscardModal(closeDiscardModalState);
	}, [updateDiscardModal]);

	const openDiscardModal = useCallback(
		(file: FileEntry) => {
			if (!isWorkingTreeReviewMode(reviewMode)) {
				return;
			}
			updateDiscardModal(() => openDiscardModalState(file));
			clearUiError();
		},
		[clearUiError, reviewMode, updateDiscardModal],
	);

	const confirmDiscardModal = useCallback(() => {
		if (!discardModal.isOpen) {
			return;
		}
		runAction(discardFileChanges(discardModal.file), renderRepoActionError, {
			onSuccess: () => {
				updateDiscardModal(closeDiscardModalState);
			},
		});
	}, [discardModal, renderRepoActionError, runAction, updateDiscardModal]);

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

	const openSelectedDiffLine = useCallback(
		(filePath: string, lineNumber: number) => {
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
			runAction(
				openFileInEditorAtLine(filePath, lineNumber),
				renderOpenFileError,
				{
					refreshOnFailure: true,
				},
			);
			renderer.resume();
		},
		[chooserFilePath, clearUiError, renderer, runAction, setUiError],
	);

	const toggleSelectedFileStage = useCallback(
		(file: FileEntry) => {
			if (!isWorkingTreeReviewMode(reviewMode)) {
				return;
			}
			runAction(toggleFileStage(file), renderRepoActionError);
		},
		[renderRepoActionError, reviewMode, runAction],
	);

	const initializeGitRepository = useCallback(() => {
		if (!canInitializeGitRepo) {
			return;
		}
		runAction(initGitRepository(), renderRepoActionError, {
			refreshOnFailure: true,
		});
	}, [canInitializeGitRepo, renderRepoActionError, runAction]);

	const resetReviewMode = useCallback(() => {
		if (isWorkingTreeReviewMode(reviewMode)) {
			return;
		}
		updateReviewMode(() => ({ _tag: "working-tree" }));
		clearUiError();
		void refreshFiles(true);
	}, [clearUiError, refreshFiles, reviewMode, updateReviewMode]);

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

	return {
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
	};
}
