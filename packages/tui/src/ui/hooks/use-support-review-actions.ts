import { Data, Effect } from "effect";
import { useCallback } from "react";
import { VigilDaemonClientContext } from "#daemon/client.ts";
import { useFrontendRuntime } from "#runtime/frontend-runtime.tsx";
import type {
	ReviewMode,
	SupportPanelTab,
	SupportReviewModalState,
	SupportReviewState,
	UpdateSupportReviewModal,
	UpdateSupportReviewState,
} from "#ui/state.ts";
import {
	beginSupportReviewGenerationState,
	closeSupportReviewModalState,
	completeSupportReviewGenerationState,
	failSupportReviewGenerationState,
	isBranchCompareReviewMode,
	openSupportReviewModalState,
	setSupportReviewActiveTabState,
} from "#ui/state.ts";

class SupportReviewRequestError extends Data.TaggedError(
	"SupportReviewRequestError",
)<{
	readonly message: string;
	readonly cause?: unknown;
}> {}

function renderUnknownError(error: unknown) {
	if (
		typeof error === "object" &&
		error !== null &&
		"message" in error &&
		typeof error.message === "string"
	) {
		return error.message;
	}

	if (error instanceof Error && error.message.length > 0) {
		return error.message;
	}

	return "Failed to generate support review.";
}

interface UseSupportReviewActionsOptions {
	readonly reviewMode: ReviewMode;
	readonly supportReviewModal: SupportReviewModalState;
	readonly supportReview: SupportReviewState;
	readonly updateSupportReviewModal: UpdateSupportReviewModal;
	readonly updateSupportReview: UpdateSupportReviewState;
	readonly clearUiError: () => void;
	readonly setUiError: (error: string) => void;
}

export function useSupportReviewActions(options: UseSupportReviewActionsOptions) {
	const runtime = useFrontendRuntime();
	const {
		reviewMode,
		supportReviewModal,
		supportReview,
		updateSupportReviewModal,
		updateSupportReview,
		clearUiError,
		setUiError,
	} = options;

	const closeSupportReviewModal = useCallback(() => {
		updateSupportReviewModal(closeSupportReviewModalState);
	}, [updateSupportReviewModal]);

	const openSupportReviewModal = useCallback(() => {
		if (supportReview.loading) {
			return;
		}
		updateSupportReviewModal(openSupportReviewModalState);
		clearUiError();
	}, [clearUiError, supportReview.loading, updateSupportReviewModal]);

	const setSupportPanelTab = useCallback(
		(activeTab: SupportPanelTab) => {
			updateSupportReview((current) =>
				setSupportReviewActiveTabState(current, activeTab),
			);
		},
		[updateSupportReview],
	);

	const confirmSupportReviewModal = useCallback(() => {
		if (!supportReviewModal.isOpen || supportReview.loading) {
			return;
		}

		const payload = isBranchCompareReviewMode(reviewMode)
			? {
					repoRoot: process.cwd(),
					mode: "branch-compare" as const,
					sourceRef: reviewMode.selection.sourceRef,
					destinationRef: reviewMode.selection.destinationRef,
				}
			: {
					repoRoot: process.cwd(),
					mode: "working-tree" as const,
					sourceRef: null,
					destinationRef: null,
				};

		updateSupportReviewModal(closeSupportReviewModalState);
		updateSupportReview(beginSupportReviewGenerationState);
		clearUiError();

		const requestReview = Effect.gen(function* () {
			const daemonClient = yield* VigilDaemonClientContext;
			const response = yield* daemonClient.support.reviewDiff({
				payload,
			}).pipe(
				Effect.mapError(
					(cause) =>
						new SupportReviewRequestError({
							message: renderUnknownError(cause),
							cause,
						}),
				),
			);

			return response.markdown;
		});

		void runtime
			.runPromise(requestReview)
			.then((markdown) => {
				updateSupportReview((current) =>
					completeSupportReviewGenerationState(current, markdown),
				);
				clearUiError();
			})
			.catch((error) => {
				const message = renderUnknownError(error);
				updateSupportReview((current) =>
					failSupportReviewGenerationState(current, message),
				);
				setUiError(message);
			});
	}, [
		clearUiError,
		reviewMode,
		runtime,
		setUiError,
		supportReview.loading,
		supportReviewModal.isOpen,
		updateSupportReview,
		updateSupportReviewModal,
	]);

	return {
		openSupportReviewModal,
		closeSupportReviewModal,
		confirmSupportReviewModal,
		setSupportPanelTab,
	};
}
