import { Effect } from "effect";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	type BranchDiffSelection,
	type CommitDiffSelection,
	type FileDiffContextLines,
	loadBranchFileContextLines,
	loadCommitFileContextLines,
	loadFileContextLines,
} from "#data/git.ts";
import {
	buildDiffHunkModel,
	buildExpandedDiffHunkBlock,
	buildExpandedDiffHunkBlockRange,
	expandDiffGap,
	type DiffGapExpansion,
	type DiffHunkGap,
	type DiffHunkModel,
} from "#diff/hunks.ts";
import type { FileEntry } from "#tui/types.ts";
import type { ReviewMode } from "#ui/state.ts";
import { isWorkingTreeReviewMode } from "#ui/state.ts";

interface DiffContextLoaders {
	readonly loadWorkingTree: (
		file: Pick<FileEntry, "path" | "status">,
	) => Effect.Effect<FileDiffContextLines, never>;
	readonly loadBranchCompare: (
		filePath: string,
		selection: BranchDiffSelection,
	) => Effect.Effect<FileDiffContextLines, never>;
	readonly loadCommitCompare: (
		filePath: string,
		selection: CommitDiffSelection,
	) => Effect.Effect<FileDiffContextLines, never>;
}

interface UseDiffExpansionStateOptions {
	readonly selectedFile: FileEntry | null;
	readonly selectedFileDiff: string;
	readonly reviewMode: ReviewMode;
	readonly loaders?: DiffContextLoaders;
}

export interface DiffDisplayGap {
	readonly gap: DiffHunkGap;
	readonly canExpandDown: boolean;
	readonly canExpandUp: boolean;
}

export interface DiffDisplayBlock {
	readonly key: string;
	readonly diff: string;
	readonly gapAfter: DiffDisplayGap | null;
}

interface UseDiffExpansionStateResult {
	readonly splitViewHunkModel: DiffHunkModel | null;
	readonly splitViewDisplayBlocks: ReadonlyArray<DiffDisplayBlock>;
	readonly onExpandGap: (gap: DiffHunkGap, direction: "up" | "down") => void;
}

const defaultLoaders: DiffContextLoaders = {
	loadWorkingTree: (file) => loadFileContextLines(file),
	loadBranchCompare: (filePath, selection) =>
		loadBranchFileContextLines(filePath, selection),
	loadCommitCompare: (filePath, selection) =>
		loadCommitFileContextLines(filePath, selection),
};

function buildReviewModeCacheKey(reviewMode: ReviewMode): string {
	if (isWorkingTreeReviewMode(reviewMode)) {
		return reviewMode._tag;
	}

	return reviewMode._tag === "branch-compare"
		? `${reviewMode._tag}:${reviewMode.selection.sourceRef}\u0000${reviewMode.selection.destinationRef}`
		: `${reviewMode._tag}:${reviewMode.selection.baseRef}\u0000${reviewMode.selection.commitHash}`;
}

function buildContextLoadEffect(
	file: FileEntry,
	reviewMode: ReviewMode,
	loaders: DiffContextLoaders,
): Effect.Effect<FileDiffContextLines, never> {
	if (isWorkingTreeReviewMode(reviewMode)) {
		return loaders.loadWorkingTree(file);
	}

	return reviewMode._tag === "branch-compare"
		? loaders.loadBranchCompare(file.path, reviewMode.selection)
		: loaders.loadCommitCompare(file.path, reviewMode.selection);
}

export function useDiffExpansionState(
	options: UseDiffExpansionStateOptions,
): UseDiffExpansionStateResult {
	const {
		loaders = defaultLoaders,
		reviewMode,
		selectedFile,
		selectedFileDiff,
	} = options;
	const [selectedFileContextLines, setSelectedFileContextLines] =
		useState<FileDiffContextLines | null>(null);
	const [gapExpansions, setGapExpansions] = useState<
		ReadonlyMap<number, DiffGapExpansion>
	>(new Map());
	const contextLinesCacheRef = useRef(new Map<string, FileDiffContextLines>());
	const inFlightLoadsRef = useRef(new Map<string, Promise<FileDiffContextLines>>());
	const selectedFileKeyRef = useRef<string | null>(null);

	const reviewModeCacheKey = useMemo(
		() => buildReviewModeCacheKey(reviewMode),
		[reviewMode],
	);
	const splitViewHunkModel = useMemo(() => {
		if (!selectedFileDiff.trim()) {
			return null;
		}

		const hunkModel = buildDiffHunkModel(selectedFileDiff);
		return hunkModel.hunks.length === 0 ? null : hunkModel;
	}, [selectedFileDiff]);
	const selectedFileKey = useMemo(() => {
		if (!selectedFile) {
			return null;
		}

		return `${reviewModeCacheKey}\u0000${selectedFile.path}\u0000${selectedFile.status}`;
	}, [reviewModeCacheKey, selectedFile]);

	selectedFileKeyRef.current = selectedFileKey;

	const ensureContextLinesLoaded = useCallback((): Promise<FileDiffContextLines> => {
		if (!selectedFile || !selectedFileKey || splitViewHunkModel === null) {
			return Promise.resolve([]);
		}

		const cachedLines = contextLinesCacheRef.current.get(selectedFileKey);
		if (cachedLines) {
			setSelectedFileContextLines(cachedLines);
			return Promise.resolve(cachedLines);
		}

		const inFlightLoad = inFlightLoadsRef.current.get(selectedFileKey);
		if (inFlightLoad) {
			return inFlightLoad;
		}

		const loadPromise = Effect.runPromise(
			buildContextLoadEffect(selectedFile, reviewMode, loaders),
		).then((lines) => {
			contextLinesCacheRef.current.set(selectedFileKey, lines);

			if (selectedFileKeyRef.current === selectedFileKey) {
				setSelectedFileContextLines(lines);
			}

			return lines;
		});

		inFlightLoadsRef.current.set(selectedFileKey, loadPromise);
		void loadPromise.finally(() => {
			const inFlightLoads = inFlightLoadsRef.current;
			if (inFlightLoads.get(selectedFileKey) === loadPromise) {
				inFlightLoads.delete(selectedFileKey);
			}
		});

		return loadPromise;
	}, [loaders, reviewMode, selectedFile, selectedFileKey, splitViewHunkModel]);

	useEffect(() => {
		setGapExpansions(new Map());
		setSelectedFileContextLines(
			selectedFileKey
				? (contextLinesCacheRef.current.get(selectedFileKey) ?? null)
				: null,
		);
	}, [selectedFileDiff, selectedFileKey]);

	useEffect(() => {
		if (!selectedFile || splitViewHunkModel === null || splitViewHunkModel.gaps.length === 0) {
			return;
		}

		void ensureContextLinesLoaded();
	}, [ensureContextLinesLoaded, selectedFile, splitViewHunkModel]);

	const onExpandGap = useCallback(
		(gap: DiffHunkGap, direction: "up" | "down") => {
			void ensureContextLinesLoaded().then((lines) => {
				if (lines.length === 0) {
					return;
				}

				setGapExpansions((current) => {
					const next = new Map(current);
					next.set(
						gap.previousHunkIndex,
						expandDiffGap(
							gap,
							direction,
							current.get(gap.previousHunkIndex),
						),
					);
					return next;
				});
			});
		},
		[ensureContextLinesLoaded],
	);

		const splitViewDisplayBlocks = useMemo(() => {
		if (splitViewHunkModel === null) {
			return [];
		}

		const gapByNextHunkIndex = new Map(
			splitViewHunkModel.gaps.map((gap) => [gap.nextHunkIndex, gap] as const),
		);
		const gapByPreviousHunkIndex = new Map(
			splitViewHunkModel.gaps.map(
				(gap) => [gap.previousHunkIndex, gap] as const,
			),
		);

		const getRemainingGapLines = (gap: DiffHunkGap): number => {
			const gapExpansion = gapExpansions.get(gap.previousHunkIndex);
			return Math.max(
				0,
				gap.newCount -
					(gapExpansion?.fromPrevious ?? 0) -
					(gapExpansion?.fromNext ?? 0),
			);
		};

		const buildDisplayBlock = (
			startIndex: number,
			endIndex: number,
			trailingGap: DiffHunkGap | null,
		): DiffDisplayBlock => {
			const hunks = splitViewHunkModel.hunks.slice(startIndex, endIndex + 1);
			const firstHunk = hunks[0];
			const lastHunk = hunks[hunks.length - 1];

			if (!firstHunk || !lastHunk) {
				return {
					key: `${startIndex}:${endIndex}`,
					diff: "",
					gapAfter: null,
				};
			}

			const previousGap = gapByNextHunkIndex.get(firstHunk.hunkIndex);
			const previousGapExpansion = previousGap
				? gapExpansions.get(previousGap.previousHunkIndex)
				: undefined;
			const trailingGapExpansion = trailingGap
				? gapExpansions.get(trailingGap.previousHunkIndex)
				: undefined;
			const hasExpandedContext =
				(previousGapExpansion?.fromNext ?? 0) > 0 ||
				(trailingGapExpansion?.fromPrevious ?? 0) > 0 ||
				hunks.length > 1;

			return {
				key: `${firstHunk.hunkIndex}:${lastHunk.hunkIndex}`,
				diff:
					selectedFileContextLines === null || !hasExpandedContext
						? firstHunk.diff
						: hunks.length === 1
							? buildExpandedDiffHunkBlock(
									splitViewHunkModel,
									firstHunk,
									selectedFileContextLines,
									previousGapExpansion,
									trailingGapExpansion,
								)
							: buildExpandedDiffHunkBlockRange(
									splitViewHunkModel,
									hunks,
									selectedFileContextLines,
									previousGapExpansion,
									trailingGapExpansion,
								),
				gapAfter:
					trailingGap && getRemainingGapLines(trailingGap) > 0
						? {
								gap: trailingGap,
								canExpandDown: true,
								canExpandUp: true,
							}
						: null,
			};
		};

		const displayBlocks: Array<DiffDisplayBlock> = [];
		let groupStartIndex = 0;

		for (let index = 1; index < splitViewHunkModel.hunks.length; index += 1) {
			const currentHunk = splitViewHunkModel.hunks[index];
			if (!currentHunk) {
				continue;
			}

			const gapBefore = gapByNextHunkIndex.get(currentHunk.hunkIndex);
			const shouldMerge =
				selectedFileContextLines !== null &&
				gapBefore !== undefined &&
				getRemainingGapLines(gapBefore) === 0;

			if (shouldMerge) {
				continue;
			}

			displayBlocks.push(
				buildDisplayBlock(groupStartIndex, index - 1, gapBefore ?? null),
			);
			groupStartIndex = index;
		}

		const lastHunk = splitViewHunkModel.hunks[splitViewHunkModel.hunks.length - 1];
		displayBlocks.push(
			buildDisplayBlock(
				groupStartIndex,
				splitViewHunkModel.hunks.length - 1,
				lastHunk
					? (gapByPreviousHunkIndex.get(lastHunk.hunkIndex) ?? null)
					: null,
			),
		);

		return displayBlocks;
	}, [gapExpansions, selectedFileContextLines, splitViewHunkModel]);

	return {
		splitViewHunkModel,
		splitViewDisplayBlocks,
		onExpandGap,
	};
}
