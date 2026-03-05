import { Effect, Option, pipe } from "effect";
import { useCallback } from "react";
import {
	type CommitDiffSelection,
	listSearchableCommits,
	resolveCommitBaseRef,
	type RepoActionError,
} from "#data/git.ts";
import { searchCommits } from "#ui/commit-search.ts";
import type {
	CommitSearchModalState,
	ReviewMode,
	UpdateCommitSearchModal,
	UpdateReviewMode,
} from "#ui/state.ts";
import {
	closeCommitSearchModalState,
	isCommitCompareReviewMode,
	openCommitSearchModalLoadingState,
} from "#ui/state.ts";

interface UseCommitSearchActionsOptions {
	readonly commitSearchModal: CommitSearchModalState;
	readonly reviewMode: ReviewMode;
	readonly updateCommitSearchModal: UpdateCommitSearchModal;
	readonly updateReviewMode: UpdateReviewMode;
	readonly clearUiError: () => void;
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly renderRepoActionError: (error: RepoActionError) => string;
}

export function useCommitSearchActions(options: UseCommitSearchActionsOptions) {
	const {
		commitSearchModal,
		reviewMode,
		updateCommitSearchModal,
		updateReviewMode,
		clearUiError,
		refreshFiles,
		renderRepoActionError,
	} = options;

	const resolveSelection = useCallback(
		(
			availableCommits: ReadonlyArray<CommitDiffSelection>,
			selectedCommitHash: Option.Option<string>,
		) => {
			const selectedCommitHashValue = Option.match(selectedCommitHash, {
				onNone: () => null,
				onSome: (value) => value,
			});
			const nextSelectedCommitHash =
				selectedCommitHashValue !== null &&
				availableCommits.some(
					(commit) => commit.commitHash === selectedCommitHashValue,
				)
					? selectedCommitHash
					: Option.fromNullable(availableCommits[0]?.commitHash);
			const selectedIndex = Option.match(nextSelectedCommitHash, {
				onNone: () => 0,
				onSome: (commitHash) =>
					Math.max(
						availableCommits.findIndex(
							(commit) => commit.commitHash === commitHash,
						),
						0,
					),
			});

			return {
				selectedCommitHash: nextSelectedCommitHash,
				selectedIndex,
			};
		},
		[],
	);

	const openCommitSearchModal = useCallback(() => {
		if (commitSearchModal.isOpen) {
			return;
		}

		const seededCommitHash = isCommitCompareReviewMode(reviewMode)
			? Option.some(reviewMode.selection.commitHash)
			: Option.none<string>();

		updateCommitSearchModal(() =>
			openCommitSearchModalLoadingState({
				selectedCommitHash: seededCommitHash,
			}),
		);

		void Effect.runPromise(
			pipe(
				listSearchableCommits(),
				Effect.match({
					onFailure: (error) => {
						updateCommitSearchModal((current) =>
							current.isOpen
								? {
										...current,
										loading: false,
										error: Option.some(renderRepoActionError(error)),
									}
								: current,
						);
					},
					onSuccess: (commits) => {
						updateCommitSearchModal((current) => {
							if (!current.isOpen) {
								return current;
							}

								const availableCommits = commits.map((commit) => ({
									commitHash: commit.hash,
									baseRef: resolveCommitBaseRef(commit),
									shortHash: commit.shortHash,
									subject: commit.subject,
								}));
								const selection = resolveSelection(
									availableCommits,
									current.selectedCommitHash,
								);

								return {
									...current,
									loading: false,
									availableCommits,
									selectedCommitHash: selection.selectedCommitHash,
									selectedIndex: selection.selectedIndex,
									error: Option.none(),
								};
							});
						},
				}),
			),
		);
	}, [
		commitSearchModal.isOpen,
		renderRepoActionError,
		reviewMode,
		resolveSelection,
		updateCommitSearchModal,
	]);

	const closeCommitSearchModal = useCallback(() => {
		updateCommitSearchModal(closeCommitSearchModalState);
	}, [updateCommitSearchModal]);

	const onCommitSearchQueryChange = useCallback(
		(query: string) => {
			updateCommitSearchModal((current) => {
					if (!current.isOpen) {
						return current;
					}
					const filtered = searchCommits(current.availableCommits, query);
					const selectedCommitHashValue = Option.match(
						current.selectedCommitHash,
						{
							onNone: () => null,
							onSome: (value) => value,
						},
					);
					const selectedCommitHash =
						selectedCommitHashValue !== null &&
						filtered.some((commit) => commit.commitHash === selectedCommitHashValue)
							? current.selectedCommitHash
							: Option.fromNullable(filtered[0]?.commitHash);
				const selectedIndex = Option.match(selectedCommitHash, {
					onNone: () => 0,
					onSome: (commitHash) =>
						Math.max(
							filtered.findIndex((commit) => commit.commitHash === commitHash),
							0,
						),
				});
				return {
					...current,
					query,
					selectedCommitHash,
					selectedIndex,
					error: Option.none(),
				};
			});
		},
		[updateCommitSearchModal],
	);

	const onCommitSearchSelectCommit = useCallback(
		(commitHash: string) => {
			updateCommitSearchModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				const filtered = searchCommits(current.availableCommits, current.query);
				const selectedIndex = Math.max(
					filtered.findIndex((commit) => commit.commitHash === commitHash),
					0,
				);
				return {
					...current,
					selectedCommitHash: Option.some(commitHash),
					selectedIndex,
					error: Option.none(),
				};
			});
		},
		[updateCommitSearchModal],
	);

	const moveCommitSearchSelection = useCallback(
		(direction: 1 | -1) => {
			updateCommitSearchModal((current) => {
				if (!current.isOpen || current.loading) {
					return current;
				}
				const filtered = searchCommits(current.availableCommits, current.query);
				if (filtered.length === 0) {
					return current;
				}

				const baseIndex = Math.min(
					Math.max(current.selectedIndex, 0),
					filtered.length - 1,
				);
				const nextIndex = (baseIndex + direction + filtered.length) % filtered.length;
				const nextCommit = filtered[nextIndex];
				if (!nextCommit) {
					return current;
				}

				return {
					...current,
					selectedCommitHash: Option.some(nextCommit.commitHash),
					selectedIndex: nextIndex,
					error: Option.none(),
				};
			});
		},
		[updateCommitSearchModal],
	);

	const confirmCommitSearchModal = useCallback(() => {
		if (!commitSearchModal.isOpen || commitSearchModal.loading) {
			return;
		}

		if (Option.isNone(commitSearchModal.selectedCommitHash)) {
			updateCommitSearchModal((current) =>
				current.isOpen
					? { ...current, error: Option.some("Select a commit.") }
					: current,
			);
			return;
		}

		const selectedCommitHashValue = Option.match(
			commitSearchModal.selectedCommitHash,
			{
				onNone: () => null,
				onSome: (value) => value,
			},
		);
		if (selectedCommitHashValue === null) {
			return;
		}

		const selectedCommit = commitSearchModal.availableCommits.find(
			(commit) => commit.commitHash === selectedCommitHashValue,
		);
		if (!selectedCommit) {
			updateCommitSearchModal((current) =>
				current.isOpen
					? { ...current, error: Option.some("Selected commit was not found.") }
					: current,
			);
			return;
		}

		updateReviewMode(() => ({
			_tag: "commit-compare",
			selection: selectedCommit,
		}));
		updateCommitSearchModal(closeCommitSearchModalState);
		clearUiError();
		void refreshFiles(true);
	}, [
		clearUiError,
		commitSearchModal,
		refreshFiles,
		updateCommitSearchModal,
		updateReviewMode,
	]);

	return {
		openCommitSearchModal,
		closeCommitSearchModal,
		confirmCommitSearchModal,
		moveCommitSearchSelection,
		onCommitSearchQueryChange,
		onCommitSearchSelectCommit,
	};
}
