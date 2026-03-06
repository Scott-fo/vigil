import { Option } from "effect";
import { useMemo } from "react";
import { searchCommits } from "#ui/commit-search.ts";
import type { CommitSearchModalState } from "#ui/state.ts";

interface UseCommitSearchViewOptions {
	readonly commitSearchModal: CommitSearchModalState;
}

export function useCommitSearchView(options: UseCommitSearchViewOptions) {
	const { commitSearchModal } = options;

	const commitSearchQuery = commitSearchModal.isOpen ? commitSearchModal.query : "";

	const commitFilteredCommits = useMemo(() => {
		if (!commitSearchModal.isOpen) {
			return [] as const;
		}

		return searchCommits(
			commitSearchModal.availableCommits,
			commitSearchModal.query,
		);
	}, [commitSearchModal]);

	const commitSelectedIndex = useMemo(() => {
		if (!commitSearchModal.isOpen) {
			return 0;
		}

		const maxIndex = Math.max(commitFilteredCommits.length - 1, 0);
		return Math.min(Math.max(commitSearchModal.selectedIndex, 0), maxIndex);
	}, [commitFilteredCommits.length, commitSearchModal]);

	const commitSelectedCommitHash = useMemo(() => {
		if (!commitSearchModal.isOpen) {
			return Option.none<string>();
		}

		const selectedByIndex = commitFilteredCommits[commitSelectedIndex];
		if (selectedByIndex) {
			return Option.some(selectedByIndex.commitHash);
		}

		const selectedCommitHashValue = Option.match(
			commitSearchModal.selectedCommitHash,
			{
				onNone: () => null,
				onSome: (value) => value,
			},
		);

		return selectedCommitHashValue !== null &&
			commitFilteredCommits.some(
				(commit) => commit.commitHash === selectedCommitHashValue,
			)
			? commitSearchModal.selectedCommitHash
			: Option.fromNullable(commitFilteredCommits[0]?.commitHash);
	}, [commitFilteredCommits, commitSearchModal, commitSelectedIndex]);

	const commitSearchModalLoading = commitSearchModal.isOpen
		? commitSearchModal.loading
		: false;

	const commitSearchModalError = commitSearchModal.isOpen
		? commitSearchModal.error
		: Option.none<string>();

	return {
		commitFilteredCommits,
		commitSearchModalError,
		commitSearchModalLoading,
		commitSearchQuery,
		commitSelectedCommitHash,
		commitSelectedIndex,
	};
}
