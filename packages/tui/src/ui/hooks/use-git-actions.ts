import { Effect, Option, pipe } from "effect";
import { useCallback } from "react";
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
import type { FileEntry } from "#tui/types";
import type {
	CommitModalState,
	DiscardModalState,
	RemoteSyncState,
	ReviewMode,
	UpdateCommitModal,
	UpdateDiscardModal,
	UpdateRemoteSyncState,
	UpdateReviewMode,
} from "#ui/state";

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

	return {
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
	};
}
