import { Effect, Option, pipe } from "effect";
import { useCallback, useEffect, useRef } from "react";
import { loadFilesWithDiffs, type RepoActionError } from "#data/git";
import type { UpdateFileViewState, UpdateUiStatus } from "#ui/state";

interface UseFileRefreshOptions {
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly renderRepoActionError: (error: RepoActionError) => string;
	readonly pollMs?: number;
}

export function useFileRefresh(options: UseFileRefreshOptions) {
	const { updateFileView, updateUiStatus, renderRepoActionError } = options;
	const isRefreshingRef = useRef(false);
	const pollMs = options.pollMs ?? 2000;

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
					loadFilesWithDiffs(),
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
				updateFileView((current) => ({
					...current,
					files: current.files.length === 0 ? current.files : [],
					selectedPath: Option.none(),
				}));
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
				const filesAreEqual =
					current.files.length === result.files.length &&
					current.files.every((file, index) => {
						const other = result.files[index];
						return other !== undefined && file.equals(other);
					});
				const nextFiles = filesAreEqual ? current.files : result.files;
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
				return {
					...current,
					files: nextFiles,
					selectedPath: nextSelectedPath,
				};
			});
			updateUiStatus((current) => {
				if (!current.showSplash && Option.isNone(current.error)) {
					return current;
				}
				return {
					showSplash: false,
					error: Option.none(),
				};
			});
		},
		[renderRepoActionError, updateFileView, updateUiStatus],
	);

	useEffect(() => {
		void refreshFiles(true);
	}, [refreshFiles]);

	useEffect(() => {
		const interval = setInterval(() => {
			void refreshFiles(false);
		}, pollMs);
		return () => clearInterval(interval);
	}, [pollMs, refreshFiles]);

	return { refreshFiles };
}
