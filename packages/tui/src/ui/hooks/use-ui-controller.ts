import { useMemo } from "react";
import {
	makeUiController,
	type UiControllerApi,
} from "#ui/services/ui-controller.ts";
import type { UpdateUiStatus } from "#ui/state.ts";

interface UseUiControllerOptions {
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly updateUiStatus: UpdateUiStatus;
}

export function useUiController(
	options: UseUiControllerOptions,
): UiControllerApi {
	return useMemo(
		() =>
			makeUiController({
				refreshFiles: options.refreshFiles,
				updateUiStatus: options.updateUiStatus,
			}),
		[options.refreshFiles, options.updateUiStatus],
	);
}
