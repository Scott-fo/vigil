import { Context, Effect, Option, pipe } from "effect";
import type { UpdateUiStatus } from "#ui/state.ts";

export interface ActionRunOptions {
	readonly refreshOnSuccess?: boolean;
	readonly refreshOnFailure?: boolean;
	readonly onSuccess?: () => void;
}

export type ActionRunResult =
	| { readonly ok: true }
	| { readonly ok: false; readonly error: string };

export interface UiControllerApi {
	readonly clearError: () => Effect.Effect<void>;
	readonly refresh: (showLoading: boolean) => Effect.Effect<void>;
	readonly run: <E>(
		effect: Effect.Effect<void, E>,
		renderError: (error: E) => string,
		options?: ActionRunOptions,
	) => Effect.Effect<ActionRunResult>;
	readonly setError: (error: string) => Effect.Effect<void>;
}

export class UiController extends Context.Tag("@vigil/tui/UiController")<
	UiController,
	UiControllerApi
>() {}

interface MakeUiControllerOptions {
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly updateUiStatus: UpdateUiStatus;
}

export function makeUiController(
	options: MakeUiControllerOptions,
): UiControllerApi {
	const clearError = Effect.fn("UiController.clearError")(function* () {
		yield* Effect.sync(() => {
			options.updateUiStatus((current) =>
				Option.isNone(current.error)
					? current
					: { ...current, error: Option.none() },
			);
		});
	});

	const setError = Effect.fn("UiController.setError")(function* (error: string) {
		yield* Effect.sync(() => {
			options.updateUiStatus((current) =>
				Option.isSome(current.error) && current.error.value === error
					? current
					: { ...current, error: Option.some(error) },
			);
		});
	});

	const refresh = Effect.fn("UiController.refresh")(function* (
		showLoading: boolean,
	) {
		yield* Effect.promise(() => options.refreshFiles(showLoading)).pipe(
			Effect.orDie,
		);
	});

	const run = <E>(
		effect: Effect.Effect<void, E>,
		renderError: (error: E) => string,
		actionOptions: ActionRunOptions = {},
	): Effect.Effect<ActionRunResult> =>
		Effect.gen(function* () {
			const refreshOnSuccess = actionOptions.refreshOnSuccess ?? true;
			const refreshOnFailure = actionOptions.refreshOnFailure ?? false;
			const result = yield* pipe(
				effect,
				Effect.match({
					onFailure: (error) => ({
						ok: false as const,
						error: renderError(error),
					}),
					onSuccess: () => ({ ok: true as const }),
				}),
			);

			if (!result.ok) {
				yield* setError(result.error);
				if (refreshOnFailure) {
					yield* refresh(false);
				}
				return result;
			}

			yield* Effect.sync(() => {
				actionOptions.onSuccess?.();
			});
			yield* clearError();
			if (refreshOnSuccess) {
				yield* refresh(false);
			}
			return result;
		}).pipe(Effect.orDie, Effect.withSpan("UiController.run"));

	return UiController.of({
		clearError,
		refresh,
		run,
		setError,
	});
}
