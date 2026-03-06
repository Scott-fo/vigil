import { useEffect, useState } from "react";
import type { FocusedPane } from "#ui/inputs.ts";

interface UsePaneNavigationStateOptions {
	readonly selectedFilePath: string | undefined;
	readonly diffLineCount: number;
}

export function usePaneNavigationState(
	options: UsePaneNavigationStateOptions,
) {
	const [activePane, setActivePane] = useState<FocusedPane>("sidebar");
	const [selectedDiffLineIndex, setSelectedDiffLineIndex] = useState(0);

	useEffect(() => {
		setSelectedDiffLineIndex(0);
	}, [options.selectedFilePath]);

	useEffect(() => {
		setSelectedDiffLineIndex((current) => {
			if (options.diffLineCount === 0) {
				return 0;
			}

			return Math.min(current, options.diffLineCount - 1);
		});
	}, [options.diffLineCount]);

	return {
		activePane,
		setActivePane,
		selectedDiffLineIndex,
		setSelectedDiffLineIndex,
	};
}
