import type { ScrollBoxRenderable } from "@opentui/core";
import { Option } from "effect";
import {
	type Dispatch,
	type RefObject,
	type SetStateAction,
	useCallback,
} from "react";
import type { FocusedPane } from "#ui/inputs.ts";
import type { UpdateFileViewState } from "#ui/state.ts";

interface UseNavigationActionsOptions {
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly diffLineCount: number;
	readonly rendererHeight: number;
	readonly sidebarOpen: boolean;
	readonly activePane: FocusedPane;
	readonly setActivePane: Dispatch<SetStateAction<FocusedPane>>;
	readonly setSelectedDiffLineIndex: Dispatch<SetStateAction<number>>;
	readonly updateFileView: UpdateFileViewState;
}

export function useNavigationActions(options: UseNavigationActionsOptions) {
	const {
		diffScrollRef,
		diffLineCount,
		rendererHeight,
		sidebarOpen,
		activePane,
		setActivePane,
		setSelectedDiffLineIndex,
		updateFileView,
	} = options;

	const toggleCollapsedDirectory = useCallback(
		(path: string) => {
			updateFileView((current) => {
				const next = new Set(current.collapsedDirectories);
				if (next.has(path)) {
					next.delete(path);
				} else {
					next.add(path);
				}

				return { ...current, collapsedDirectories: next };
			});
		},
		[updateFileView],
	);

	const toggleSidebar = useCallback(() => {
		if (sidebarOpen && activePane === "sidebar") {
			setActivePane("diff");
		}

		updateFileView((current) => ({
			...current,
			sidebarOpen: !current.sidebarOpen,
		}));
	}, [activePane, setActivePane, sidebarOpen, updateFileView]);

	const focusSidebarPane = useCallback(() => {
		setActivePane("sidebar");
		updateFileView((current) =>
			current.sidebarOpen ? current : { ...current, sidebarOpen: true },
		);
	}, [setActivePane, updateFileView]);

	const focusDiffPane = useCallback(() => {
		setActivePane("diff");
	}, [setActivePane]);

	const toggleDiffViewMode = useCallback(() => {
		updateFileView((current) => ({
			...current,
			diffViewMode: current.diffViewMode === "split" ? "unified" : "split",
		}));
	}, [updateFileView]);

	const selectFilePath = useCallback(
		(path: string) => {
			updateFileView((current) => ({
				...current,
				selectedPath: Option.some(path),
			}));
		},
		[updateFileView],
	);

	const scrollDiffHalfPage = useCallback(
		(direction: "up" | "down") => {
			const diffScroll = diffScrollRef.current;
			if (!diffScroll) {
				return;
			}

			const step = Math.max(6, Math.floor(rendererHeight * 0.45));
			diffScroll.scrollBy({
				x: 0,
				y: direction === "up" ? -step : step,
			});

			if (activePane !== "diff" || diffLineCount <= 0) {
				return;
			}

			const topVisibleLine = Math.max(0, Math.floor(diffScroll.scrollTop));
			setSelectedDiffLineIndex(Math.min(topVisibleLine, diffLineCount - 1));
		},
		[
			activePane,
			diffLineCount,
			diffScrollRef,
			rendererHeight,
			setSelectedDiffLineIndex,
		],
	);

	const moveDiffSelection = useCallback(
		(direction: 1 | -1) => {
			if (activePane !== "diff" || diffLineCount <= 0) {
				return;
			}

			setSelectedDiffLineIndex((current) =>
				Math.max(0, Math.min(current + direction, diffLineCount - 1)),
			);
		},
		[activePane, diffLineCount, setSelectedDiffLineIndex],
	);

	return {
		focusDiffPane,
		focusSidebarPane,
		moveDiffSelection,
		onSelectFilePath: selectFilePath,
		onToggleDirectory: toggleCollapsedDirectory,
		onToggleSidebar: toggleSidebar,
		scrollDiffHalfPage,
		toggleDiffViewMode,
	};
}
