import { Option } from "effect";
import { useMemo } from "react";
import { searchBranchRefs } from "#ui/branch-ref-search.ts";
import type {
	BranchCompareField,
	BranchCompareModalState,
} from "#ui/state.ts";

interface UseBranchCompareViewOptions {
	readonly branchCompareModal: BranchCompareModalState;
}

export function useBranchCompareView(options: UseBranchCompareViewOptions) {
	const { branchCompareModal } = options;

	const branchSourceQuery = branchCompareModal.isOpen
		? branchCompareModal.sourceQuery
		: "";

	const branchDestinationQuery = branchCompareModal.isOpen
		? branchCompareModal.destinationQuery
		: "";

	const branchSourceRef = branchCompareModal.isOpen
		? branchCompareModal.sourceRef
		: Option.none<string>();

	const branchDestinationRef = branchCompareModal.isOpen
		? branchCompareModal.destinationRef
		: Option.none<string>();

	const branchActiveField: BranchCompareField = branchCompareModal.isOpen
		? branchCompareModal.activeField
		: "source";

	const branchFilteredRefs = useMemo(() => {
		if (!branchCompareModal.isOpen) {
			return [] as const;
		}

		const query =
			branchCompareModal.activeField === "source"
				? branchCompareModal.sourceQuery
				: branchCompareModal.destinationQuery;

		return searchBranchRefs(branchCompareModal.availableRefs, query);
	}, [branchCompareModal]);

	const branchSelectedActiveRef = useMemo(() => {
		if (!branchCompareModal.isOpen) {
			return Option.none<string>();
		}

		const selectedIndex =
			branchCompareModal.activeField === "source"
				? branchCompareModal.selectedSourceIndex
				: branchCompareModal.selectedDestinationIndex;

		const selectedByIndex = branchFilteredRefs[selectedIndex];
		if (selectedByIndex) {
			return Option.some(selectedByIndex);
		}

		const selectedRef =
			branchCompareModal.activeField === "source"
				? branchCompareModal.sourceRef
				: branchCompareModal.destinationRef;

		return Option.isSome(selectedRef) &&
			branchFilteredRefs.includes(selectedRef.value)
			? selectedRef
			: Option.fromNullable(branchFilteredRefs[0]);
	}, [branchCompareModal, branchFilteredRefs]);

	const branchModalLoading = branchCompareModal.isOpen
		? branchCompareModal.loading
		: false;

	const branchModalError = branchCompareModal.isOpen
		? branchCompareModal.error
		: Option.none<string>();

	return {
		branchActiveField,
		branchDestinationQuery,
		branchDestinationRef,
		branchFilteredRefs,
		branchModalError,
		branchModalLoading,
		branchSelectedActiveRef,
		branchSourceQuery,
		branchSourceRef,
	};
}
