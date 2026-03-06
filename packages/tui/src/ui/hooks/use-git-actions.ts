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
	ActionRunOptions,
	ActionRunResult,
	UiControllerApi,
} from "#ui/services/ui-controller.ts";
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
	readonly renderRepoActionError: (error: RepoActionError) => string;
	readonly uiController: UiControllerApi;
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
		renderRepoActionError,
		uiController,
	} = options;

	const submitCommit = useCallback(
		(rawMessage: string) => {
			const result = Effect.runSync(
				uiController.run(commitStagedChanges(rawMessage), renderRepoActionError, {
					onSuccess: () => {
						updateCommitModal(closeCommitModalState);
					},
				}),
			);
			if (!result.ok) {
				updateCommitModal((current) =>
					setCommitModalErrorState(current, result.error),
				);
			}
		},
		[renderRepoActionError, uiController, updateCommitModal],
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
		Effect.runSync(uiController.clearError());
	}, [reviewMode, stagedFileCount, uiController, updateCommitModal]);

	const closeDiscardModal = useCallback(() => {
		updateDiscardModal(closeDiscardModalState);
	}, [updateDiscardModal]);

	const openDiscardModal = useCallback(
		(file: FileEntry) => {
			if (!isWorkingTreeReviewMode(reviewMode)) {
				return;
			}
			updateDiscardModal(() => openDiscardModalState(file));
			Effect.runSync(uiController.clearError());
		},
		[reviewMode, uiController, updateDiscardModal],
	);

	const confirmDiscardModal = useCallback(() => {
		if (!discardModal.isOpen) {
			return;
		}
		Effect.runSync(
			uiController.run(discardFileChanges(discardModal.file), renderRepoActionError, {
				onSuccess: () => {
					updateDiscardModal(closeDiscardModalState);
				},
			}),
		);
	}, [discardModal, renderRepoActionError, uiController, updateDiscardModal]);

	const openSelectedFile = useCallback(
		(filePath: string) => {
			if (Option.isSome(chooserFilePath)) {
				void Effect.runPromise(
					pipe(
						writeChooserSelection(chooserFilePath.value, filePath),
						Effect.match({
							onFailure: (error) =>
								uiController.setError(renderOpenFileError(error)),
							onSuccess: () =>
								pipe(
									uiController.clearError(),
									Effect.tap(() => Effect.sync(() => {
										renderer.destroy();
									})),
								),
						}),
						Effect.flatten,
					),
				);
				return;
			}

			renderer.suspend();
			Effect.runSync(
				uiController.run(openFileInEditor(filePath), renderOpenFileError, {
					refreshOnFailure: true,
				}),
			);
			renderer.resume();
		},
		[chooserFilePath, renderer, uiController],
	);

	const openSelectedDiffLine = useCallback(
		(filePath: string, lineNumber: number) => {
			if (Option.isSome(chooserFilePath)) {
				void Effect.runPromise(
					pipe(
						writeChooserSelection(chooserFilePath.value, filePath),
						Effect.match({
							onFailure: (error) =>
								uiController.setError(renderOpenFileError(error)),
							onSuccess: () =>
								pipe(
									uiController.clearError(),
									Effect.tap(() => Effect.sync(() => {
										renderer.destroy();
									})),
								),
						}),
						Effect.flatten,
					),
				);
				return;
			}

			renderer.suspend();
			Effect.runSync(
				uiController.run(openFileInEditorAtLine(filePath, lineNumber), renderOpenFileError, {
					refreshOnFailure: true,
				}),
			);
			renderer.resume();
		},
		[chooserFilePath, renderer, uiController],
	);

	const toggleSelectedFileStage = useCallback(
		(file: FileEntry) => {
			if (!isWorkingTreeReviewMode(reviewMode)) {
				return;
			}
			Effect.runSync(uiController.run(toggleFileStage(file), renderRepoActionError));
		},
		[renderRepoActionError, reviewMode, uiController],
	);

	const initializeGitRepository = useCallback(() => {
		if (!canInitializeGitRepo) {
			return;
		}
		Effect.runSync(
			uiController.run(initGitRepository(), renderRepoActionError, {
				refreshOnFailure: true,
			}),
		);
	}, [canInitializeGitRepo, renderRepoActionError, uiController]);

	const resetReviewMode = useCallback(() => {
		if (isWorkingTreeReviewMode(reviewMode)) {
			return;
		}
		updateReviewMode(() => ({ _tag: "working-tree" }));
		void Effect.runPromise(
			pipe(uiController.clearError(), Effect.zipRight(uiController.refresh(true))),
		);
	}, [reviewMode, uiController, updateReviewMode]);

	const syncRemote = useCallback(
		(direction: "pull" | "push") => {
			if (remoteSync._tag === "running") {
				return;
			}

			updateRemoteSync(() => ({
				_tag: "running",
				direction,
			}));
			Effect.runSync(uiController.clearError());

			void Effect.runPromise(
				pipe(
					direction === "push" ? pushToRemote() : pullFromRemote(),
					Effect.match({
						onFailure: (error) =>
							uiController.setError(renderRepoActionError(error)),
						onSuccess: () =>
							pipe(
								uiController.clearError(),
								Effect.zipRight(uiController.refresh(false)),
							),
					}),
					Effect.flatten,
					Effect.ensuring(
						Effect.sync(() => {
							updateRemoteSync(() => ({ _tag: "idle" }));
						}),
					),
				),
			);
		},
		[
			remoteSync,
			renderRepoActionError,
			uiController,
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
