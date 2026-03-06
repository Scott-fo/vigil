import type { ScrollBoxRenderable } from "@opentui/core";
import { Effect, Option, pipe } from "effect";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	loadBlameCommitDetails,
	type BlameCommitDetails,
	type RepoActionError,
} from "#data/git.ts";
import type { BlameTarget } from "#tui/types.ts";
import type { UpdateReviewMode, UpdateUiStatus } from "#ui/state.ts";

interface BlameViewState {
	readonly isOpen: boolean;
	readonly target: BlameTarget | null;
	readonly loading: boolean;
	readonly details: BlameCommitDetails | null;
	readonly error: string | null;
}

interface UseBlameViewOptions {
	readonly initialTarget: Option.Option<BlameTarget>;
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly renderRepoActionError: (error: RepoActionError) => string;
	readonly updateReviewMode: UpdateReviewMode;
	readonly updateUiStatus: UpdateUiStatus;
}

function createBlameViewState(
	initialTarget: Option.Option<BlameTarget>,
): BlameViewState {
	return Option.match(initialTarget, {
		onNone: () => ({
			isOpen: false,
			target: null,
			loading: false,
			details: null,
			error: null,
		}),
		onSome: (target) => ({
			isOpen: true,
			target,
			loading: true,
			details: null,
			error: null,
		}),
	});
}

export function useBlameView(options: UseBlameViewOptions) {
	const {
		initialTarget,
		refreshFiles,
		renderRepoActionError,
		updateReviewMode,
		updateUiStatus,
	} = options;
	const [blameView, setBlameView] = useState<BlameViewState>(() =>
		createBlameViewState(initialTarget),
	);
	const scrollRef = useRef<ScrollBoxRenderable | null>(null);

	useEffect(() => {
		if (!blameView.isOpen || !blameView.loading || !blameView.target) {
			return;
		}

		const target = blameView.target;
		let cancelled = false;

		void Effect.runPromise(
			pipe(
				loadBlameCommitDetails(target),
				Effect.match({
					onFailure: (error) => ({
						ok: false as const,
						error: renderRepoActionError(error),
					}),
					onSuccess: (details) => ({
						ok: true as const,
						details,
					}),
				}),
			),
		).then((result) => {
			if (cancelled) {
				return;
			}

			setBlameView((current) => {
				if (
					!current.isOpen ||
					!current.loading ||
					!current.target ||
					current.target.filePath !== target.filePath ||
					current.target.lineNumber !== target.lineNumber
				) {
					return current;
				}

				return result.ok
					? {
							...current,
							loading: false,
							details: result.details,
							error: null,
						}
					: {
							...current,
							loading: false,
							details: null,
							error: result.error,
						};
			});
		});

		return () => {
			cancelled = true;
		};
	}, [blameView.isOpen, blameView.loading, blameView.target, renderRepoActionError]);

	const canOpenCommitCompare = useMemo(
		() =>
			blameView.isOpen &&
			blameView.details !== null &&
			Option.isSome(blameView.details.compareSelection),
		[blameView],
	);

	const close = useCallback(() => {
		setBlameView((current) =>
			current.isOpen ? { ...current, isOpen: false } : current,
		);
	}, []);

	const openCommitCompare = useCallback(() => {
		if (!blameView.isOpen || !blameView.details) {
			return;
		}

		const compareSelection = blameView.details.compareSelection;
		if (Option.isNone(compareSelection)) {
			setBlameView((current) =>
				current.isOpen
					? {
							...current,
							error: "No committed change is available for this line.",
						}
					: current,
			);
			return;
		}

		updateReviewMode(() => ({
			_tag: "commit-compare",
			selection: compareSelection.value,
		}));
		updateUiStatus((current) =>
			Option.isNone(current.error)
				? current
				: { ...current, error: Option.none() },
		);
		setBlameView((current) =>
			current.isOpen ? { ...current, isOpen: false } : current,
		);
		void refreshFiles(true);
	}, [blameView, refreshFiles, updateReviewMode, updateUiStatus]);

	const scroll = useCallback((direction: "up" | "down") => {
		const blameScroll = scrollRef.current;
		if (!blameScroll) {
			return;
		}

		blameScroll.scrollBy({
			x: 0,
			y: direction === "up" ? -3 : 3,
		});
	}, []);

	return {
		blameView,
		canOpenCommitCompare,
		close,
		openCommitCompare,
		scroll,
		scrollRef,
	};
}
