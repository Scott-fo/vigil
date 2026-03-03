import { Effect, Option, pipe } from "effect";
import { useCallback } from "react";
import { listComparableRefs, type RepoActionError } from "#data/git";
import type {
	BranchCompareField,
	BranchCompareModalState,
	ReviewMode,
	UpdateBranchCompareModal,
	UpdateReviewMode,
} from "#ui/state";
import {
	closeBranchCompareModalState,
	isBranchCompareReviewMode,
	openBranchCompareModalLoadingState,
} from "#ui/state";
import { searchBranchRefs } from "#ui/branch-ref-search";

interface UseBranchCompareActionsOptions {
	readonly branchCompareModal: BranchCompareModalState;
	readonly reviewMode: ReviewMode;
	readonly updateBranchCompareModal: UpdateBranchCompareModal;
	readonly updateReviewMode: UpdateReviewMode;
	readonly clearUiError: () => void;
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly renderRepoActionError: (error: RepoActionError) => string;
}

function resolveDestinationRef(
	refs: ReadonlyArray<string>,
	sourceRef: Option.Option<string>,
): Option.Option<string> {
	const sourceValue = Option.match(sourceRef, {
		onNone: () => undefined,
		onSome: (value) => value,
	});
	const preferred = refs.find(
		(refName) =>
			(refName === "main" || refName === "master") && refName !== sourceValue,
	);
	if (preferred) {
		return Option.some(preferred);
	}
	const firstDifferent = refs.find((refName) => refName !== sourceValue);
	if (firstDifferent) {
		return Option.some(firstDifferent);
	}
	return Option.fromNullable(refs[0]);
}

export function useBranchCompareActions(options: UseBranchCompareActionsOptions) {
	const {
		branchCompareModal,
		reviewMode,
		updateBranchCompareModal,
		updateReviewMode,
		clearUiError,
		refreshFiles,
		renderRepoActionError,
	} = options;

	const openBranchCompareModal = useCallback(() => {
		if (branchCompareModal.isOpen) {
			return;
		}

		const seededSourceRef =
			isBranchCompareReviewMode(reviewMode)
				? Option.some(reviewMode.selection.sourceRef)
				: Option.none<string>();
		const seededDestinationRef =
			isBranchCompareReviewMode(reviewMode)
				? Option.some(reviewMode.selection.destinationRef)
				: Option.none<string>();

		updateBranchCompareModal(() =>
			openBranchCompareModalLoadingState({
				sourceRef: seededSourceRef,
				destinationRef: seededDestinationRef,
			}),
		);

		void Effect.runPromise(
			pipe(
				listComparableRefs(),
				Effect.match({
					onFailure: (error) => {
						updateBranchCompareModal((current) =>
							current.isOpen
								? {
										...current,
										loading: false,
										error: Option.some(renderRepoActionError(error)),
									}
								: current,
						);
					},
					onSuccess: (refs) => {
						updateBranchCompareModal((current) => {
							if (!current.isOpen) {
								return current;
							}

							const sourceRef =
								Option.isSome(current.sourceRef) &&
								refs.includes(current.sourceRef.value)
									? current.sourceRef
									: Option.fromNullable(refs[0]);
							const destinationRef =
								Option.isSome(current.destinationRef) &&
								refs.includes(current.destinationRef.value)
									? current.destinationRef
									: resolveDestinationRef(refs, sourceRef);
							const selectedSourceIndex = Option.match(sourceRef, {
								onNone: () => 0,
								onSome: (refName) => Math.max(refs.indexOf(refName), 0),
							});
							const selectedDestinationIndex = Option.match(destinationRef, {
								onNone: () => 0,
								onSome: (refName) => Math.max(refs.indexOf(refName), 0),
							});

							return {
								...current,
								loading: false,
								availableRefs: refs,
								sourceRef,
								destinationRef,
								selectedSourceIndex,
								selectedDestinationIndex,
								error: Option.none(),
							};
						});
					},
				}),
			),
		);
	}, [
		branchCompareModal.isOpen,
		renderRepoActionError,
		reviewMode,
		updateBranchCompareModal,
	]);

	const closeBranchCompareModal = useCallback(() => {
		updateBranchCompareModal(closeBranchCompareModalState);
	}, [updateBranchCompareModal]);

	const onBranchActivateField = useCallback(
		(field: BranchCompareField) => {
			updateBranchCompareModal((current) =>
				current.isOpen ? { ...current, activeField: field } : current,
			);
		},
		[updateBranchCompareModal],
	);

	const updateBranchQuery = useCallback(
		(field: BranchCompareField, query: string) => {
			updateBranchCompareModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				const filtered = searchBranchRefs(current.availableRefs, query);
				const currentRef =
					field === "source" ? current.sourceRef : current.destinationRef;
				const nextRef =
					Option.isSome(currentRef) && filtered.includes(currentRef.value)
						? currentRef
						: Option.fromNullable(filtered[0]);
				const nextIndex = Option.match(nextRef, {
					onNone: () => 0,
					onSome: (refName) => Math.max(filtered.indexOf(refName), 0),
				});
				return field === "source"
					? {
							...current,
							sourceQuery: query,
							sourceRef: nextRef,
							selectedSourceIndex: nextIndex,
							error: Option.none(),
						}
					: {
							...current,
							destinationQuery: query,
							destinationRef: nextRef,
							selectedDestinationIndex: nextIndex,
							error: Option.none(),
						};
			});
		},
		[updateBranchCompareModal],
	);

	const onBranchSourceQueryChange = useCallback(
		(value: string) => {
			updateBranchQuery("source", value);
		},
		[updateBranchQuery],
	);

	const onBranchDestinationQueryChange = useCallback(
		(value: string) => {
			updateBranchQuery("destination", value);
		},
		[updateBranchQuery],
	);

	const onBranchSelectRef = useCallback(
		(refName: string) => {
			updateBranchCompareModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				const activeQuery =
					current.activeField === "source"
						? current.sourceQuery
						: current.destinationQuery;
				const filtered = searchBranchRefs(current.availableRefs, activeQuery);
				const nextIndex = Math.max(filtered.indexOf(refName), 0);

				return current.activeField === "source"
					? {
							...current,
							sourceRef: Option.some(refName),
							sourceQuery: refName,
							selectedSourceIndex: nextIndex,
							error: Option.none(),
						}
					: {
							...current,
							destinationRef: Option.some(refName),
							destinationQuery: refName,
							selectedDestinationIndex: nextIndex,
							error: Option.none(),
						};
			});
		},
		[updateBranchCompareModal],
	);

	const moveBranchSelection = useCallback(
		(direction: 1 | -1) => {
			updateBranchCompareModal((current) => {
				if (!current.isOpen || current.loading) {
					return current;
				}
				const activeQuery =
					current.activeField === "source"
						? current.sourceQuery
						: current.destinationQuery;
				const filtered = searchBranchRefs(current.availableRefs, activeQuery);
				if (filtered.length === 0) {
					return current;
				}
				const selectedIndex =
					current.activeField === "source"
						? current.selectedSourceIndex
						: current.selectedDestinationIndex;
				const baseIndex = Math.min(
					Math.max(selectedIndex, 0),
					filtered.length - 1,
				);
				const nextIndex = (baseIndex + direction + filtered.length) % filtered.length;
				const nextRef = filtered[nextIndex];
				if (!nextRef) {
					return current;
				}

				return current.activeField === "source"
					? {
							...current,
							sourceRef: Option.some(nextRef),
							selectedSourceIndex: nextIndex,
							error: Option.none(),
						}
					: {
							...current,
							destinationRef: Option.some(nextRef),
							selectedDestinationIndex: nextIndex,
							error: Option.none(),
						};
			});
		},
		[updateBranchCompareModal],
	);

	const switchBranchField = useCallback(() => {
		updateBranchCompareModal((current) => {
			if (!current.isOpen) {
				return current;
			}
			return {
				...current,
				activeField:
					current.activeField === "source" ? "destination" : "source",
			};
		});
	}, [updateBranchCompareModal]);

	const confirmBranchCompareModal = useCallback(() => {
		if (!branchCompareModal.isOpen || branchCompareModal.loading) {
			return;
		}

		if (
			Option.isNone(branchCompareModal.sourceRef) ||
			Option.isNone(branchCompareModal.destinationRef)
		) {
			updateBranchCompareModal((current) =>
				current.isOpen
					? {
							...current,
							error: Option.some("Select both source and destination refs."),
						}
					: current,
			);
			return;
		}

		const sourceRef = branchCompareModal.sourceRef.value;
		const destinationRef = branchCompareModal.destinationRef.value;
		if (sourceRef === destinationRef) {
			updateBranchCompareModal((current) =>
				current.isOpen
					? {
							...current,
							error: Option.some(
								"Source and destination refs must be different.",
							),
						}
					: current,
			);
			return;
		}

		updateReviewMode(() => ({
			_tag: "branch-compare",
			selection: {
				sourceRef,
				destinationRef,
			},
		}));
		updateBranchCompareModal(closeBranchCompareModalState);
		clearUiError();
		void refreshFiles(true);
	}, [
		branchCompareModal,
		clearUiError,
		refreshFiles,
		updateBranchCompareModal,
		updateReviewMode,
	]);

	return {
		openBranchCompareModal,
		closeBranchCompareModal,
		confirmBranchCompareModal,
		moveBranchSelection,
		switchBranchField,
		onBranchSourceQueryChange,
		onBranchDestinationQueryChange,
		onBranchSelectRef,
		onBranchActivateField,
	};
}
