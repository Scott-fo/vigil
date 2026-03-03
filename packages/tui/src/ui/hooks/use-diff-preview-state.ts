import { Effect, Option } from "effect";
import { useEffect, useMemo, useRef, useState } from "react";
import {
	loadBranchFilePreview,
	loadFilePreview,
	type FileDiffPreview,
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

interface UseDiffPreviewStateOptions {
	readonly files: ReadonlyArray<FileEntry>;
	readonly selectedFile: FileEntry | null;
	readonly reviewMode: ReviewMode;
	readonly externalRefreshVersion?: number;
}

interface UseDiffPreviewStateResult {
	readonly selectedFileDiff: string;
	readonly selectedFileDiffNote: Option.Option<string>;
	readonly selectedFileDiffLoading: boolean;
}

export function useDiffPreviewState(
	options: UseDiffPreviewStateOptions,
): UseDiffPreviewStateResult {
	const { files, selectedFile, reviewMode } = options;
	const externalRefreshVersion = options.externalRefreshVersion ?? 0;
	const [selectedFilePreview, setSelectedFilePreview] =
		useState<SelectedFilePreview | null>(null);
	const filePreviewCacheRef = useRef(
		new Map<
			string,
			{
				readonly status: string;
				readonly preview: FileDiffPreview;
			}
		>(),
	);

	useEffect(() => {
		filePreviewCacheRef.current.clear();
		setSelectedFilePreview(null);
	}, [reviewMode]);

	useEffect(() => {
		if (!selectedFile) {
			setSelectedFilePreview(null);
			return;
		}

		const cachedPreview = filePreviewCacheRef.current.get(selectedFile.path);

		if (cachedPreview && cachedPreview.status === selectedFile.status) {
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

		let cancelled = false;
		const previewEffect = isWorkingTreeReviewMode(reviewMode)
			? loadFilePreview(selectedFile)
			: loadBranchFilePreview(selectedFile.path, reviewMode.selection);

		void Effect.runPromise(previewEffect).then((preview) => {
			if (cancelled) {
				return;
			}
			filePreviewCacheRef.current.set(selectedFile.path, {
				status: selectedFile.status,
				preview,
			});
			setSelectedFilePreview({
				path: selectedFile.path,
				status: selectedFile.status,
				loading: false,
				preview,
			});
		});

		return () => {
			cancelled = true;
		};
	}, [externalRefreshVersion, reviewMode, selectedFile]);

	useEffect(() => {
		const visiblePaths = new Set(files.map((file) => file.path));
		const cache = filePreviewCacheRef.current;
		for (const [path] of cache) {
			if (!visiblePaths.has(path)) {
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
