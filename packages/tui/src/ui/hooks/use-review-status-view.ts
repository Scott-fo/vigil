import { Option, pipe } from "effect";
import type { ReviewMode, UiStatus } from "#ui/state.ts";
import { isCommitCompareReviewMode, isWorkingTreeReviewMode } from "#ui/state.ts";

interface UseReviewStatusViewOptions {
	readonly reviewMode: ReviewMode;
	readonly uiStatus: UiStatus;
}

export function useReviewStatusView(options: UseReviewStatusViewOptions) {
	const { reviewMode, uiStatus } = options;

	const canInitializeGitRepo = pipe(
		uiStatus.error,
		Option.match({
			onNone: () => false,
			onSome: (error) =>
				uiStatus.showSplash && /not a git repository/i.test(error),
		}),
	);

	const reviewModeLabel = isWorkingTreeReviewMode(reviewMode)
		? ""
		: isCommitCompareReviewMode(reviewMode)
			? `Commit ${reviewMode.selection.shortHash}: ${reviewMode.selection.subject}`
			: `Compare ${reviewMode.selection.sourceRef} -> ${reviewMode.selection.destinationRef}`;

	return {
		canInitializeGitRepo,
		reviewModeLabel,
	};
}
