import { Effect, Option, pipe } from "effect";
import { useCallback } from "react";
import type { UpdateUiStatus } from "#ui/state.ts";

interface UseActionRunnerOptions {
	readonly updateUiStatus: UpdateUiStatus;
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
}

export interface ActionRunOptions {
	readonly refreshOnSuccess?: boolean;
	readonly refreshOnFailure?: boolean;
	readonly onSuccess?: () => void;
}

export type ActionRunResult =
	| { readonly ok: true }
	| { readonly ok: false; readonly error: string };

export function useActionRunner(options: UseActionRunnerOptions) {
	const { updateUiStatus, refreshFiles } = options;

	const clearUiError = useCallback(() => {
		updateUiStatus((current) =>
			Option.isNone(current.error)
				? current
				: { ...current, error: Option.none() },
		);
	}, [updateUiStatus]);

	const setUiError = useCallback(
		(error: string) => {
			updateUiStatus((current) =>
				Option.isSome(current.error) && current.error.value === error
					? current
					: { ...current, error: Option.some(error) },
			);
		},
		[updateUiStatus],
	);

	const runAction = useCallback(
		<E>(
			effect: Effect.Effect<void, E>,
			renderError: (error: E) => string,
			actionOptions: ActionRunOptions = {},
		): ActionRunResult => {
			const refreshOnSuccess = actionOptions.refreshOnSuccess ?? true;
			const refreshOnFailure = actionOptions.refreshOnFailure ?? false;
			const result = Effect.runSync(
				pipe(
					effect,
					Effect.match({
						onFailure: (error) => ({
							ok: false as const,
							error: renderError(error),
						}),
						onSuccess: () => ({ ok: true as const }),
					}),
				),
			);

			if (!result.ok) {
				setUiError(result.error);
				if (refreshOnFailure) {
					void refreshFiles(false);
				}
				return { ok: false, error: result.error };
			}

			actionOptions.onSuccess?.();
			clearUiError();
			if (refreshOnSuccess) {
				void refreshFiles(false);
			}
			return { ok: true };
		},
		[clearUiError, refreshFiles, setUiError],
	);

	return {
		clearUiError,
		runAction,
		setUiError,
	};
}
