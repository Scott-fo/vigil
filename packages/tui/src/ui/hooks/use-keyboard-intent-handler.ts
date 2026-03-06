import { useCallback } from "react";
import {
	routeKeyboardIntent,
	type KeyboardIntentRouterActions,
} from "#ui/hooks/keyboard-intent-router.ts";
import type { AppKeyboardIntent } from "#ui/inputs.ts";

interface UseKeyboardIntentHandlerOptions
	extends KeyboardIntentRouterActions {}

export function useKeyboardIntentHandler(
	options: UseKeyboardIntentHandlerOptions,
) {
	const onKeyboardIntent = useCallback(
		(intent: AppKeyboardIntent) => routeKeyboardIntent(intent, options),
		[options],
	);

	return { onKeyboardIntent };
}
