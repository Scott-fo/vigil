import { Effect, Option } from "effect";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	type BranchDiffSelection,
	type CommitDiffSelection,
	type FileDiffPreview,
	loadBranchFilePreview,
	loadCommitFilePreview,
	loadFilePreview,
} from "#data/git.ts";
import type { FileEntry } from "#tui/types.ts";
import type { ReviewMode } from "#ui/state.ts";
import { isWorkingTreeReviewMode } from "#ui/state.ts";

interface SelectedFilePreview {
	readonly path: string;
	readonly status: string;
	readonly loading: boolean;
	readonly preview: FileDiffPreview;
}

interface DiffPreviewCacheEntry {
	readonly status: string;
	readonly preview: FileDiffPreview;
	readonly refreshVersion: number;
}

export interface DiffPreviewLoaders {
	readonly loadWorkingTree: (
		file: Pick<FileEntry, "path" | "status">,
	) => Effect.Effect<FileDiffPreview, never>;
	readonly loadBranchCompare: (
		filePath: string,
		selection: BranchDiffSelection,
	) => Effect.Effect<FileDiffPreview, never>;
	readonly loadCommitCompare: (
		filePath: string,
		selection: CommitDiffSelection,
	) => Effect.Effect<FileDiffPreview, never>;
}

interface UseDiffPreviewStateOptions {
	readonly files: ReadonlyArray<FileEntry>;
	readonly visibleFilePaths: ReadonlyArray<string>;
	readonly selectedFile: FileEntry | null;
	readonly selectedVisibleIndex: number;
	readonly reviewMode: ReviewMode;
	readonly externalRefreshVersion?: number;
	readonly loaders?: DiffPreviewLoaders;
}

interface UseDiffPreviewStateResult {
	readonly selectedFileDiff: string;
	readonly selectedFileDiffNote: Option.Option<string>;
	readonly selectedFileDiffLoading: boolean;
}

const DIFF_PREFETCH_RADIUS = 3;
const DIFF_PREFETCH_CONCURRENCY = 6;

const defaultDiffPreviewLoaders: DiffPreviewLoaders = {
	loadWorkingTree: (file) => loadFilePreview(file),
	loadBranchCompare: (filePath, selection) =>
		loadBranchFilePreview(filePath, selection),
	loadCommitCompare: (filePath, selection) =>
		loadCommitFilePreview(filePath, selection),
};

function isCachedPreviewAvailable(
	entry: DiffPreviewCacheEntry | undefined,
	status: string,
): entry is DiffPreviewCacheEntry {
	return entry?.status === status;
}

function isCachedPreviewFresh(
	entry: DiffPreviewCacheEntry | undefined,
	status: string,
	refreshVersion: number,
): entry is DiffPreviewCacheEntry {
	return (
		isCachedPreviewAvailable(entry, status) &&
		entry.refreshVersion === refreshVersion
	);
}

function buildPreviewRequestKey(
	path: string,
	status: string,
	refreshVersion: number,
): string {
	return `${path}\u0000${status}\u0000${refreshVersion}`;
}

export function buildDiffPrefetchPaths(
	visibleFilePaths: ReadonlyArray<string>,
	selectedVisibleIndex: number,
	radius = DIFF_PREFETCH_RADIUS,
): ReadonlyArray<string> {
	if (
		selectedVisibleIndex < 0 ||
		selectedVisibleIndex >= visibleFilePaths.length ||
		radius < 0
	) {
		return [];
	}

	const orderedPaths: Array<string> = [];
	const selectedPath = visibleFilePaths[selectedVisibleIndex];

	if (selectedPath) {
		orderedPaths.push(selectedPath);
	}

	for (let offset = 1; offset <= radius; offset += 1) {
		const previousPath = visibleFilePaths[selectedVisibleIndex - offset];
		if (previousPath) {
			orderedPaths.push(previousPath);
		}

		const nextPath = visibleFilePaths[selectedVisibleIndex + offset];
		if (nextPath) {
			orderedPaths.push(nextPath);
		}
	}

	return orderedPaths;
}

function buildPreviewLoadEffect(
	file: Pick<FileEntry, "path" | "status">,
	reviewMode: ReviewMode,
	loaders: DiffPreviewLoaders,
): Effect.Effect<FileDiffPreview, never> {
	if (isWorkingTreeReviewMode(reviewMode)) {
		return loaders.loadWorkingTree(file);
	}

	return reviewMode._tag === "branch-compare"
		? loaders.loadBranchCompare(file.path, reviewMode.selection)
		: loaders.loadCommitCompare(file.path, reviewMode.selection);
}

export function useDiffPreviewState(
	options: UseDiffPreviewStateOptions,
): UseDiffPreviewStateResult {
	const {
		files,
		loaders = defaultDiffPreviewLoaders,
		reviewMode,
		selectedFile,
		selectedVisibleIndex,
		visibleFilePaths,
	} = options;
	const externalRefreshVersion = options.externalRefreshVersion ?? 0;
	const [selectedFilePreview, setSelectedFilePreview] =
		useState<SelectedFilePreview | null>(null);
	const filePreviewCacheRef = useRef(new Map<string, DiffPreviewCacheEntry>());
	const inFlightPreviewLoadsRef = useRef(new Map<string, Promise<void>>());
	const cacheEpochRef = useRef(0);
	const selectedFileRef = useRef<FileEntry | null>(selectedFile);

	selectedFileRef.current = selectedFile;

	const reviewModeCacheKey = useMemo(() => {
		if (isWorkingTreeReviewMode(reviewMode)) {
			return reviewMode._tag;
		}

		return reviewMode._tag === "branch-compare"
			? `${reviewMode._tag}:${reviewMode.selection.sourceRef}\u0000${reviewMode.selection.destinationRef}`
			: `${reviewMode._tag}:${reviewMode.selection.baseRef}\u0000${reviewMode.selection.commitHash}`;
	}, [reviewMode]);

	const fileByPath = useMemo(
		() => new Map(files.map((file) => [file.path, file] as const)),
		[files],
	);

	const startPreviewLoad = useCallback(
		(file: FileEntry): Promise<void> => {
			const cachedPreview = filePreviewCacheRef.current.get(file.path);
			if (
				isCachedPreviewFresh(cachedPreview, file.status, externalRefreshVersion)
			) {
				return Promise.resolve();
			}

			const requestKey = buildPreviewRequestKey(
				file.path,
				file.status,
				externalRefreshVersion,
			);
			const inFlightPreview = inFlightPreviewLoadsRef.current.get(requestKey);
			if (inFlightPreview) {
				return inFlightPreview;
			}

			const cacheEpoch = cacheEpochRef.current;
			const loadPromise = Effect.runPromise(
				buildPreviewLoadEffect(file, reviewMode, loaders),
			)
				.then((preview) => {
					if (cacheEpochRef.current !== cacheEpoch) {
						return;
					}

					filePreviewCacheRef.current.set(file.path, {
						status: file.status,
						preview,
						refreshVersion: externalRefreshVersion,
					});

					const currentSelectedFile = selectedFileRef.current;
					if (
						currentSelectedFile?.path !== file.path ||
						currentSelectedFile.status !== file.status
					) {
						return;
					}

					setSelectedFilePreview({
						path: file.path,
						status: file.status,
						loading: false,
						preview,
					});
				})
				.finally(() => {
					const inFlightLoads = inFlightPreviewLoadsRef.current;
					if (inFlightLoads.get(requestKey) === loadPromise) {
						inFlightLoads.delete(requestKey);
					}
				});

			inFlightPreviewLoadsRef.current.set(requestKey, loadPromise);
			return loadPromise;
		},
		[externalRefreshVersion, loaders, reviewMode],
	);

	useEffect(() => {
		if (reviewModeCacheKey.length === 0) {
			return;
		}

		cacheEpochRef.current += 1;
		filePreviewCacheRef.current.clear();
		inFlightPreviewLoadsRef.current.clear();
		setSelectedFilePreview(null);
	}, [reviewModeCacheKey]);

	useEffect(() => {
		if (!selectedFile) {
			setSelectedFilePreview(null);
			return;
		}

		const cachedPreview = filePreviewCacheRef.current.get(selectedFile.path);
		if (isCachedPreviewAvailable(cachedPreview, selectedFile.status)) {
			setSelectedFilePreview({
				path: selectedFile.path,
				status: selectedFile.status,
				loading: false,
				preview: cachedPreview.preview,
			});
		} else {
			setSelectedFilePreview({
				path: selectedFile.path,
				status: selectedFile.status,
				loading: true,
				preview: { diff: "", note: Option.none() },
			});
		}

		const prefetchPaths = buildDiffPrefetchPaths(
			visibleFilePaths,
			selectedVisibleIndex,
		);
		const prefetchFiles = prefetchPaths.flatMap((path) => {
			const file = fileByPath.get(path);
			return file ? [file] : [];
		});

		void Effect.runPromise(
			Effect.forEach(
				prefetchFiles,
				(file) => Effect.promise(() => startPreviewLoad(file)),
				{
					concurrency: DIFF_PREFETCH_CONCURRENCY,
					discard: true,
				},
			),
		);
	}, [
		fileByPath,
		selectedFile,
		selectedVisibleIndex,
		startPreviewLoad,
		visibleFilePaths,
	]);

	useEffect(() => {
		const validPaths = new Set(files.map((file) => file.path));
		const cache = filePreviewCacheRef.current;
		for (const [path] of cache) {
			if (!validPaths.has(path)) {
				cache.delete(path);
			}
		}
	}, [files]);

	return useMemo(() => {
		if (!selectedFile) {
			return {
				selectedFileDiff: "",
				selectedFileDiffNote: Option.none<string>(),
				selectedFileDiffLoading: false,
			};
		}

		if (
			!selectedFilePreview ||
			selectedFilePreview.path !== selectedFile.path ||
			selectedFilePreview.status !== selectedFile.status
		) {
			return {
				selectedFileDiff: "",
				selectedFileDiffNote: Option.none<string>(),
				selectedFileDiffLoading: true,
			};
		}

		return {
			selectedFileDiff: selectedFilePreview.preview.diff,
			selectedFileDiffNote: selectedFilePreview.preview.note,
			selectedFileDiffLoading: selectedFilePreview.loading,
		};
	}, [selectedFile, selectedFilePreview]);
}
