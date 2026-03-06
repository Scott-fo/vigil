import { Option, pipe } from "effect";
import { useMemo } from "react";
import { isFileStaged } from "#data/git.ts";
import type { SidebarItem } from "#ui/sidebar.ts";
import { buildSidebarTree, flattenSidebarTree } from "#ui/sidebar.ts";
import type { FileViewState } from "#ui/state.ts";

interface UseReviewFileViewOptions {
	readonly fileView: FileViewState;
}

export function useReviewFileView(options: UseReviewFileViewOptions) {
	const {
		files,
		sidebarOpen,
		diffViewMode,
		collapsedDirectories,
		selectedPath,
		loading,
	} = options.fileView;

	const fileByPath = useMemo(
		() => new Map(files.map((file) => [file.path, file] as const)),
		[files],
	);

	const selectedFile = useMemo(() => {
		if (files.length === 0) {
			return null;
		}

		const selectedFileMatch = pipe(
			selectedPath,
			Option.flatMap((path) => Option.fromNullable(fileByPath.get(path))),
		);

		return pipe(
			selectedFileMatch,
			Option.getOrElse(() => files[0] ?? null),
		);
	}, [fileByPath, files, selectedPath]);

	const sidebarTree = useMemo(() => buildSidebarTree(files), [files]);

	const sidebarItems = useMemo(
		() => flattenSidebarTree(sidebarTree, collapsedDirectories),
		[collapsedDirectories, sidebarTree],
	);

	const visibleFilePaths = useMemo(
		() =>
			sidebarItems
				.filter(
					(item): item is Extract<SidebarItem, { kind: "file" }> =>
						item.kind === "file",
				)
				.map((item) => item.file.path),
		[sidebarItems],
	);

	const visibleFileIndexByPath = useMemo(
		() =>
			new Map(visibleFilePaths.map((path, index) => [path, index] as const)),
		[visibleFilePaths],
	);

	const selectedVisibleIndex = useMemo(() => {
		if (!selectedFile) {
			return -1;
		}

		return visibleFileIndexByPath.get(selectedFile.path) ?? -1;
	}, [selectedFile, visibleFileIndexByPath]);

	const stagedFileCount = useMemo(
		() => files.filter((file) => isFileStaged(file.status)).length,
		[files],
	);

	return {
		diffViewMode,
		files,
		loading,
		selectedFile,
		selectedVisibleIndex,
		sidebarItems,
		sidebarOpen,
		stagedFileCount,
		visibleFilePaths,
	};
}
