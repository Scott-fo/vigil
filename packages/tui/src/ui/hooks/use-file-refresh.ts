import { Effect, Option, pipe } from "effect";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	type BranchDiffSelection,
	type CommitDiffSelection,
	loadFilesWithBranchDiffs,
	loadFilesWithCommitDiff,
	loadFilesWithStatus,
	type RepoActionError,
} from "#data/git.ts";
import type {
	ReviewMode,
	UpdateFileViewState,
	UpdateUiStatus,
} from "#ui/state.ts";
import { isWorkingTreeReviewMode } from "#ui/state.ts";

interface UseFileRefreshOptions {
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly renderRepoActionError: (error: RepoActionError) => string;
	readonly reviewMode: ReviewMode;
}

export interface RefreshRequestState {
	readonly isRefreshing: boolean;
	readonly hasQueuedRefresh: boolean;
	readonly queuedShowLoading: boolean;
}

export function registerRefreshRequest(
	state: RefreshRequestState,
	showLoading: boolean,
): {
	readonly nextState: RefreshRequestState;
	readonly shouldRunNow: boolean;
} {
	if (!state.isRefreshing) {
		return {
			nextState: state,
			shouldRunNow: true,
		};
	}
	return {
		nextState: {
			...state,
			hasQueuedRefresh: true,
			queuedShowLoading: state.queuedShowLoading || showLoading,
		},
		shouldRunNow: false,
	};
}

export function consumeQueuedRefresh(state: RefreshRequestState): {
	readonly nextState: RefreshRequestState;
	readonly queuedShowLoading: Option.Option<boolean>;
} {
	if (!state.hasQueuedRefresh) {
		return {
			nextState: state,
			queuedShowLoading: Option.none(),
		};
	}
	return {
		nextState: {
			...state,
			hasQueuedRefresh: false,
			queuedShowLoading: false,
		},
		queuedShowLoading: Option.some(state.queuedShowLoading),
	};
}

export interface FileRefreshLoaders {
	readonly loadWorkingTree: () => ReturnType<typeof loadFilesWithStatus>;
	readonly loadBranchCompare: (
		selection: BranchDiffSelection,
	) => ReturnType<typeof loadFilesWithBranchDiffs>;
	readonly loadCommitCompare: (
		selection: CommitDiffSelection,
	) => ReturnType<typeof loadFilesWithCommitDiff>;
}

const defaultFileRefreshLoaders: FileRefreshLoaders = {
	loadWorkingTree: () => loadFilesWithStatus(),
	loadBranchCompare: (selection) => loadFilesWithBranchDiffs(selection),
	loadCommitCompare: (selection) => loadFilesWithCommitDiff(selection),
};

export function buildFilesLoadEffect(
	reviewMode: ReviewMode,
	loaders: FileRefreshLoaders = defaultFileRefreshLoaders,
) {
	if (isWorkingTreeReviewMode(reviewMode)) {
		return loaders.loadWorkingTree();
	}
	return reviewMode._tag === "branch-compare"
		? loaders.loadBranchCompare(reviewMode.selection)
		: loaders.loadCommitCompare(reviewMode.selection);
}

export function useFileRefresh(options: UseFileRefreshOptions) {
	const { updateFileView, updateUiStatus, renderRepoActionError, reviewMode } =
		options;
	const [refreshInstructionVersion, setRefreshInstructionVersion] = useState(0);
	const isRefreshingRef = useRef(false);
	const queuedRefreshRef = useRef(false);
	const queuedShowLoadingRef = useRef(false);
	const requestStateRef = useRef<RefreshRequestState>({
		isRefreshing: false,
		hasQueuedRefresh: false,
		queuedShowLoading: false,
	});
	const reviewModeRef = useRef(reviewMode);
	reviewModeRef.current = reviewMode;

	const runRefreshEffect = useCallback(
		(showLoading: boolean) =>
			Effect.gen(function* () {
				let nextShowLoading = showLoading;
				let shouldContinue = true;

				while (shouldContinue) {
					yield* Effect.sync(() => {
						isRefreshingRef.current = true;
						requestStateRef.current = {
							...requestStateRef.current,
							isRefreshing: true,
						};
						if (nextShowLoading) {
							updateFileView((current) =>
								current.loading ? current : { ...current, loading: true },
							);
						}
					});

					const result = yield* pipe(
						buildFilesLoadEffect(reviewModeRef.current),
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
								if (nextShowLoading) {
									updateFileView((current) =>
										current.loading ? { ...current, loading: false } : current,
									);
								}
								isRefreshingRef.current = false;
								requestStateRef.current = {
									...requestStateRef.current,
									isRefreshing: false,
								};
							}),
						),
					);

					if (!result.ok) {
						const queuedShowLoading = yield* Effect.sync(() => {
							updateFileView((current) => {
								if (
									current.files.length === 0 &&
									Option.isNone(current.selectedPath)
								) {
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

							const queuedRefresh = consumeQueuedRefresh(requestStateRef.current);
							requestStateRef.current = queuedRefresh.nextState;
							return queuedRefresh.queuedShowLoading;
						});

						if (Option.isSome(queuedShowLoading)) {
							nextShowLoading = queuedShowLoading.value;
							yield* Effect.sync(() => {
								queuedRefreshRef.current = false;
								queuedShowLoadingRef.current = false;
							});
							continue;
						}

						return;
					}

					const queuedShowLoading = yield* Effect.sync(() => {
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
									onSome: (path) =>
										result.files.some((file) => file.path === path),
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

						const queuedRefresh = consumeQueuedRefresh(requestStateRef.current);
						requestStateRef.current = queuedRefresh.nextState;
						return queuedRefresh.queuedShowLoading;
					});

					if (Option.isSome(queuedShowLoading)) {
						nextShowLoading = queuedShowLoading.value;
						yield* Effect.sync(() => {
							queuedRefreshRef.current = false;
							queuedShowLoadingRef.current = false;
						});
						continue;
					}

					shouldContinue = false;
				}
			}),
		[renderRepoActionError, updateFileView, updateUiStatus],
	);

	const refreshFilesEffect = useCallback(
		(showLoading: boolean) =>
			Effect.gen(function* () {
				const request = registerRefreshRequest(
					requestStateRef.current,
					showLoading,
				);
				requestStateRef.current = request.nextState;

				if (!request.shouldRunNow) {
					queuedRefreshRef.current = true;
					queuedShowLoadingRef.current = request.nextState.queuedShowLoading;
					return;
				}

				yield* runRefreshEffect(showLoading);
			}),
		[runRefreshEffect],
	);

	const refreshFiles = useCallback(
		(showLoading: boolean) => Effect.runPromise(refreshFilesEffect(showLoading)),
		[refreshFilesEffect],
	);

	const onRefreshInstruction = useMemo(
		() =>
			pipe(
				refreshFilesEffect(false),
				Effect.tap(() =>
					Effect.sync(() => {
						setRefreshInstructionVersion((current) => current + 1);
					}),
				),
			),
		[refreshFilesEffect],
	);

	useEffect(() => {
		void Effect.runPromise(refreshFilesEffect(true));
	}, [refreshFilesEffect, reviewMode]);

	return {
		refreshFiles,
		refreshFilesEffect,
		refreshInstructionVersion,
		onRefreshInstruction,
	};
}
