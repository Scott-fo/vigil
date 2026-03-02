import { Effect, Option, pipe } from "effect";
import { useCallback, useEffect, useRef } from "react";
import {
	loadFilesWithBranchDiffs,
	loadFilesWithStatus,
	type RepoActionError,
} from "#data/git";
import type { ReviewMode, UpdateFileViewState, UpdateUiStatus } from "#ui/state";

interface UseFileRefreshOptions {
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly renderRepoActionError: (error: RepoActionError) => string;
	readonly reviewMode: ReviewMode;
	readonly pollMs?: number;
	readonly pollingEnabled?: boolean;
}

export function useFileRefresh(options: UseFileRefreshOptions) {
	const { updateFileView, updateUiStatus, renderRepoActionError, reviewMode } =
		options;
	const isRefreshingRef = useRef(false);
	const pollMs = options.pollMs ?? 2000;
	const pollingEnabled = options.pollingEnabled ?? true;

	const refreshFiles = useCallback(
		async (showLoading: boolean) => {
			if (isRefreshingRef.current) {
				return;
			}

			isRefreshingRef.current = true;
			if (showLoading) {
				updateFileView((current) =>
					current.loading ? current : { ...current, loading: true },
				);
			}

			const result = await Effect.runPromise(
				pipe(
					reviewMode._tag === "working-tree"
						? loadFilesWithStatus()
						: loadFilesWithBranchDiffs(reviewMode.selection),
					Effect.match({
						onFailure: (repoError) => ({
							ok: false as const,
							error: renderRepoActionError(repoError),
						}),
						onSuccess: (files) => ({
							ok: true as const,
							files,
						}),
					}),
					Effect.ensuring(
						Effect.sync(() => {
							if (showLoading) {
								updateFileView((current) =>
									current.loading ? { ...current, loading: false } : current,
								);
							}
							isRefreshingRef.current = false;
						}),
					),
				),
			);

			if (!result.ok) {
				updateFileView((current) => {
					if (current.files.length === 0 && Option.isNone(current.selectedPath)) {
						return current;
					}
					return {
						...current,
						files: current.files.length === 0 ? current.files : [],
						selectedPath: Option.none(),
					};
				});
				updateUiStatus((current) => {
					if (
						current.showSplash &&
						Option.isSome(current.error) &&
						current.error.value === result.error
					) {
						return current;
					}
					return {
						showSplash: true,
						error: Option.some(result.error),
					};
				});
				return;
			}

			updateFileView((current) => {
				const previousByPath = new Map(
					current.files.map((file) => [file.path, file] as const),
				);
				const nextFiles = result.files.map((file) => {
					const previous = previousByPath.get(file.path);
					if (!previous) {
						return file;
					}
					return previous.equals(file) ? previous : file;
				});
				const filesAreEqual =
					current.files.length === nextFiles.length &&
					current.files.every((file, index) => file === nextFiles[index]);
				const hasCurrentSelection = pipe(
					current.selectedPath,
					Option.match({
						onNone: () => false,
						onSome: (path) => result.files.some((file) => file.path === path),
					}),
				);
				const nextSelectedPath =
					result.files.length === 0
						? Option.none<string>()
						: hasCurrentSelection
							? current.selectedPath
							: pipe(
									Option.fromNullable(result.files[0]),
									Option.map((file) => file.path),
								);
				const selectedPathUnchanged =
					(Option.isNone(current.selectedPath) &&
						Option.isNone(nextSelectedPath)) ||
					(Option.isSome(current.selectedPath) &&
						Option.isSome(nextSelectedPath) &&
						current.selectedPath.value === nextSelectedPath.value);
				if (filesAreEqual && selectedPathUnchanged) {
					return current;
				}
				return {
					...current,
					files: nextFiles,
					selectedPath: nextSelectedPath,
				};
			});
			updateUiStatus((current) => {
				const shouldShowSplash = result.files.length === 0;
				if (
					current.showSplash === shouldShowSplash &&
					Option.isNone(current.error)
				) {
					return current;
				}
				return {
					showSplash: shouldShowSplash,
					error: Option.none(),
				};
			});
		},
		[renderRepoActionError, reviewMode, updateFileView, updateUiStatus],
	);

	useEffect(() => {
		void refreshFiles(true);
	}, [refreshFiles]);

	useEffect(() => {
		if (!pollingEnabled) {
			return;
		}
		const interval = setInterval(() => {
			void refreshFiles(false);
		}, pollMs);
		return () => clearInterval(interval);
	}, [pollMs, pollingEnabled, refreshFiles]);

	return { refreshFiles };
}
