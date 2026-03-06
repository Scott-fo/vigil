import type {
	BranchCompareModalState,
	CommitSearchModalState,
	CommitModalState,
	DiscardModalState,
	HelpModalState,
	ThemeModalState,
} from "#ui/state.ts";
import { deriveModalVisibility } from "#ui/state.ts";

interface UseModalViewOptions {
	readonly branchCompareModal: BranchCompareModalState;
	readonly commitModal: CommitModalState;
	readonly commitSearchModal: CommitSearchModalState;
	readonly discardModal: DiscardModalState;
	readonly helpModal: HelpModalState;
	readonly themeModal: ThemeModalState;
}

export function useModalView(options: UseModalViewOptions) {
	const visibility = deriveModalVisibility(options);

	return {
		...visibility,
		discardModalFile: options.discardModal.isOpen
			? options.discardModal.file
			: null,
	};
}
