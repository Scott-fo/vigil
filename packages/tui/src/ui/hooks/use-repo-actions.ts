import type { ScrollBoxRenderable } from "@opentui/core";
import { Effect, Match, Option, pipe } from "effect";
import {
	type Dispatch,
	type RefObject,
	type SetStateAction,
	useCallback,
} from "react";
import {
	openFileInEditor,
	renderOpenFileError,
	writeChooserSelection,
} from "#data/editor";
import {
	commitStagedChanges,
	discardFileChanges,
	initGitRepository,
	listComparableRefs,
	pullFromRemote,
	pushToRemote,
	type RepoActionError,
	toggleFileStage,
} from "#data/git";
import {
	persistThemePreferenceToTuiConfig,
	type ThemeCatalog,
	type ThemeMode,
	type ThemePreferencePersistError,
} from "#theme/theme";
import type { FileEntry } from "#tui/types";
import type { AppKeyboardIntent } from "#ui/inputs";
import type {
	BranchCompareField,
	BranchCompareModalState,
	CommitModalState,
	DiscardModalState,
	HelpModalState,
	ReviewMode,
	ThemeModalState,
	RemoteSyncState,
	UpdateBranchCompareModal,
	UpdateCommitModal,
	UpdateDiscardModal,
	UpdateFileViewState,
	UpdateHelpModal,
	UpdateReviewMode,
	UpdateRemoteSyncState,
	UpdateThemeModal,
	UpdateUiStatus,
} from "#ui/state";

interface RendererControls {
	readonly height: number;
	destroy(): void;
	suspend(): void;
	resume(): void;
}

interface UseRepoActionsOptions {
	readonly chooserFilePath: Option.Option<string>;
	readonly renderer: RendererControls;
	readonly diffScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly themeName: string;
	readonly themeMode: ThemeMode;
	readonly themeCatalog: ThemeCatalog;
	readonly themeModalThemeNames: ReadonlyArray<string>;
	readonly setThemeName: Dispatch<SetStateAction<string>>;
	readonly stagedFileCount: number;
	readonly commitModal: CommitModalState;
	readonly discardModal: DiscardModalState;
	readonly helpModal: HelpModalState;
	readonly themeModal: ThemeModalState;
	readonly branchCompareModal: BranchCompareModalState;
	readonly remoteSync: RemoteSyncState;
	readonly reviewMode: ReviewMode;
	readonly canInitializeGitRepo: boolean;
	readonly updateFileView: UpdateFileViewState;
	readonly updateUiStatus: UpdateUiStatus;
	readonly updateCommitModal: UpdateCommitModal;
	readonly updateDiscardModal: UpdateDiscardModal;
	readonly updateHelpModal: UpdateHelpModal;
	readonly updateThemeModal: UpdateThemeModal;
	readonly updateBranchCompareModal: UpdateBranchCompareModal;
	readonly updateRemoteSync: UpdateRemoteSyncState;
	readonly updateReviewMode: UpdateReviewMode;
	readonly refreshFiles: (showLoading: boolean) => Promise<void>;
	readonly renderRepoActionError: (error: RepoActionError) => string;
}

interface RunActionOptions {
	readonly refreshOnSuccess?: boolean;
	readonly refreshOnFailure?: boolean;
	readonly onSuccess?: () => void;
}

type RunActionResult =
	| { readonly ok: true }
	| { readonly ok: false; readonly error: string };

export function useRepoActions(options: UseRepoActionsOptions) {
	const {
		chooserFilePath,
		renderer,
		diffScrollRef,
		themeName,
		themeMode,
		themeCatalog,
		themeModalThemeNames,
		setThemeName,
		stagedFileCount,
		commitModal,
		discardModal,
		helpModal,
		themeModal,
		branchCompareModal,
		remoteSync,
		reviewMode,
		canInitializeGitRepo,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		updateDiscardModal,
		updateHelpModal,
		updateThemeModal,
		updateBranchCompareModal,
		updateRemoteSync,
		updateReviewMode,
		refreshFiles,
		renderRepoActionError,
	} = options;

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
		<E,>(
			effect: Effect.Effect<void, E>,
			renderError: (error: E) => string,
			actionOptions: RunActionOptions = {},
		): RunActionResult => {
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

	const renderThemePreferencePersistError = useCallback(
		(error: ThemePreferencePersistError) =>
			Match.value(error).pipe(
				Match.tag(
					"ThemePreferenceConfigParseError",
					() => "Invalid theme config. Fix it and try again.",
				),
				Match.tag("ThemePreferenceConfigReadError", (typedError) =>
					typedError.message,
				),
				Match.tag("ThemePreferenceConfigWriteError", (typedError) =>
					typedError.message,
				),
				Match.exhaustive,
			),
		[],
	);

	const submitCommit = useCallback(
		(rawMessage: string) => {
			const result = runAction(
				commitStagedChanges(rawMessage),
				renderRepoActionError,
				{
					onSuccess: () => {
						updateCommitModal((current) =>
							current.isOpen ? { isOpen: false } : current,
						);
					},
				},
			);
			if (!result.ok) {
				updateCommitModal((current) =>
					current.isOpen
						? { ...current, error: Option.some(result.error) }
						: current,
				);
			}
		},
		[renderRepoActionError, runAction, updateCommitModal],
	);

	const openSelectedFile = useCallback(
		(filePath: string) => {
			if (Option.isSome(chooserFilePath)) {
				void Effect.runPromise(
					pipe(
						writeChooserSelection(chooserFilePath.value, filePath),
						Effect.match({
							onFailure: (error) => {
								setUiError(renderOpenFileError(error));
							},
							onSuccess: () => {
								clearUiError();
								renderer.destroy();
							},
						}),
					),
				);
				return;
			}

			renderer.suspend();
			runAction(openFileInEditor(filePath), renderOpenFileError, {
				refreshOnFailure: true,
			});
			renderer.resume();
		},
		[chooserFilePath, clearUiError, renderer, runAction, setUiError],
	);

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
		updateFileView((current) => ({
			...current,
			sidebarOpen: !current.sidebarOpen,
		}));
	}, [updateFileView]);

	const toggleDiffViewMode = useCallback(() => {
		updateFileView((current) => ({
			...current,
			diffViewMode:
				current.diffViewMode === "split" ? "unified" : "split",
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

	const onCommitMessageChange = useCallback(
		(value: string) => {
			updateCommitModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				return {
					...current,
					message: value,
					error: Option.none(),
				};
			});
		},
		[updateCommitModal],
	);

	const onCommitSubmit = useCallback(
		(payload: unknown) => {
			if (typeof payload === "string") {
				submitCommit(payload);
				return;
			}
			if (!commitModal.isOpen) {
				return;
			}
			submitCommit(commitModal.message);
		},
		[commitModal, submitCommit],
	);

	const closeCommitModal = useCallback(() => {
		updateCommitModal((current) =>
			current.isOpen ? { isOpen: false } : current,
		);
	}, [updateCommitModal]);

	const closeDiscardModal = useCallback(() => {
		updateDiscardModal((current) =>
			current.isOpen ? { isOpen: false } : current,
		);
	}, [updateDiscardModal]);

	const openDiscardModal = useCallback(
		(file: FileEntry) => {
			if (reviewMode._tag !== "working-tree") {
				return;
			}
			updateDiscardModal(() => ({
				isOpen: true,
				file,
			}));
			clearUiError();
		},
		[clearUiError, reviewMode._tag, updateDiscardModal],
	);

	const confirmDiscardModal = useCallback(() => {
		if (!discardModal.isOpen) {
			return;
		}
		runAction(
			discardFileChanges(discardModal.file),
			renderRepoActionError,
			{
				onSuccess: () => {
					updateDiscardModal(() => ({ isOpen: false }));
				},
			},
		);
	}, [discardModal, renderRepoActionError, runAction, updateDiscardModal]);

	const openCommitModal = useCallback(() => {
		if (reviewMode._tag !== "working-tree") {
			return;
		}
		if (stagedFileCount === 0) {
			return;
		}
		updateCommitModal(() => ({
			isOpen: true,
			message: "",
			error: Option.none(),
		}));
		clearUiError();
	}, [clearUiError, reviewMode._tag, stagedFileCount, updateCommitModal]);

	const closeHelpModal = useCallback(() => {
		updateHelpModal((current) =>
			current.isOpen ? { isOpen: false } : current,
		);
	}, [updateHelpModal]);

	const openHelpModal = useCallback(() => {
		if (helpModal.isOpen) {
			return;
		}
		updateHelpModal(() => ({ isOpen: true }));
	}, [helpModal.isOpen, updateHelpModal]);

	const openThemeModal = useCallback(() => {
		if (themeModal.isOpen) {
			return;
		}
		updateThemeModal(() => ({
			isOpen: true,
			initialThemeName: themeName,
			selectedThemeName: themeName,
		}));
	}, [themeModal.isOpen, themeName, updateThemeModal]);

	const closeThemeModal = useCallback(() => {
		if (!themeModal.isOpen) {
			return;
		}
		setThemeName(themeModal.initialThemeName);
		updateThemeModal(() => ({ isOpen: false }));
	}, [setThemeName, themeModal, updateThemeModal]);

	const confirmThemeModal = useCallback(() => {
		if (!themeModal.isOpen) {
			return;
		}
		const nextThemeName = themeModal.selectedThemeName;
		setThemeName(nextThemeName);
		updateThemeModal(() => ({ isOpen: false }));
		void Effect.runPromise(
			pipe(
				persistThemePreferenceToTuiConfig({
					theme: nextThemeName,
					mode: themeMode,
				}),
				Effect.match({
					onFailure: (error) => {
						setUiError(renderThemePreferencePersistError(error));
					},
					onSuccess: () => {
						clearUiError();
					},
				}),
			),
		);
	}, [
		clearUiError,
		renderThemePreferencePersistError,
		setThemeName,
		setUiError,
		themeModal,
		themeMode,
		updateThemeModal,
	]);

	const moveThemeSelection = useCallback(
		(direction: 1 | -1) => {
			if (!themeModal.isOpen) {
				return;
			}
			if (themeModalThemeNames.length === 0) {
				return;
			}
			const currentIndex = themeModalThemeNames.indexOf(
				themeModal.selectedThemeName,
			);
			const baseIndex = currentIndex === -1 ? 0 : currentIndex;
			const nextIndex =
				(baseIndex + direction + themeModalThemeNames.length) %
				themeModalThemeNames.length;
			const nextThemeName =
				themeModalThemeNames[nextIndex] ?? themeModal.selectedThemeName;
			if (nextThemeName === themeModal.selectedThemeName) {
				return;
			}
			setThemeName(nextThemeName);
			updateThemeModal((current) =>
				current.isOpen
					? { ...current, selectedThemeName: nextThemeName }
					: current,
			);
		},
		[setThemeName, themeModal, themeModalThemeNames, updateThemeModal],
	);

	const selectThemeInModal = useCallback(
		(nextThemeName: string) => {
			if (!themeModal.isOpen) {
				return;
			}
			if (!themeCatalog.themes[nextThemeName]) {
				return;
			}
			setThemeName(nextThemeName);
			updateThemeModal((current) =>
				current.isOpen
					? { ...current, selectedThemeName: nextThemeName }
					: current,
			);
		},
		[setThemeName, themeCatalog.themes, themeModal.isOpen, updateThemeModal],
	);

	const filterComparableRefs = useCallback(
		(refs: ReadonlyArray<string>, query: string): ReadonlyArray<string> => {
			const normalizedQuery = query.trim().toLowerCase();
			if (normalizedQuery.length === 0) {
				return refs;
			}
			return refs.filter((refName) =>
				refName.toLowerCase().includes(normalizedQuery),
			);
		},
		[],
	);

	const resolveDestinationRef = useCallback(
		(
			refs: ReadonlyArray<string>,
			sourceRef: Option.Option<string>,
		): Option.Option<string> => {
			const sourceValue = Option.match(sourceRef, {
				onNone: () => undefined,
				onSome: (value) => value,
			});
			const preferred = refs.find(
				(refName) =>
					(refName === "main" || refName === "master") && refName !== sourceValue,
			);
			if (preferred) {
				return Option.some(preferred);
			}
			const firstDifferent = refs.find((refName) => refName !== sourceValue);
			if (firstDifferent) {
				return Option.some(firstDifferent);
			}
			return Option.fromNullable(refs[0]);
		},
		[],
	);

	const openBranchCompareModal = useCallback(() => {
		if (branchCompareModal.isOpen) {
			return;
		}

		const seededSourceRef =
			reviewMode._tag === "branch-compare"
				? Option.some(reviewMode.selection.sourceRef)
				: Option.none<string>();
		const seededDestinationRef =
			reviewMode._tag === "branch-compare"
				? Option.some(reviewMode.selection.destinationRef)
				: Option.none<string>();

		updateBranchCompareModal(() => ({
			isOpen: true,
			loading: true,
			availableRefs: [],
			sourceQuery: "",
			destinationQuery: "",
			sourceRef: seededSourceRef,
			destinationRef: seededDestinationRef,
			activeField: "source",
			selectedSourceIndex: 0,
			selectedDestinationIndex: 0,
			error: Option.none(),
		}));

		void Effect.runPromise(
			pipe(
				listComparableRefs(),
				Effect.match({
					onFailure: (error) => {
						updateBranchCompareModal((current) =>
							current.isOpen
								? {
										...current,
										loading: false,
										error: Option.some(renderRepoActionError(error)),
									}
								: current,
						);
					},
					onSuccess: (refs) => {
						updateBranchCompareModal((current) => {
							if (!current.isOpen) {
								return current;
							}

							const sourceRef =
								Option.isSome(current.sourceRef) &&
								refs.includes(current.sourceRef.value)
									? current.sourceRef
									: Option.fromNullable(refs[0]);
							const destinationRef =
								Option.isSome(current.destinationRef) &&
								refs.includes(current.destinationRef.value)
									? current.destinationRef
									: resolveDestinationRef(refs, sourceRef);
							const selectedSourceIndex = Option.match(sourceRef, {
								onNone: () => 0,
								onSome: (refName) => Math.max(refs.indexOf(refName), 0),
							});
							const selectedDestinationIndex = Option.match(destinationRef, {
								onNone: () => 0,
								onSome: (refName) => Math.max(refs.indexOf(refName), 0),
							});

							return {
								...current,
								loading: false,
								availableRefs: refs,
								sourceRef,
								destinationRef,
								selectedSourceIndex,
								selectedDestinationIndex,
								error: Option.none(),
							};
						});
					},
				}),
			),
		);
	}, [
		branchCompareModal.isOpen,
		renderRepoActionError,
		resolveDestinationRef,
		reviewMode,
		updateBranchCompareModal,
	]);

	const closeBranchCompareModal = useCallback(() => {
		updateBranchCompareModal((current) =>
			current.isOpen ? { isOpen: false } : current,
		);
	}, [updateBranchCompareModal]);

	const activateBranchField = useCallback(
		(field: BranchCompareField) => {
			updateBranchCompareModal((current) =>
				current.isOpen ? { ...current, activeField: field } : current,
			);
		},
		[updateBranchCompareModal],
	);

	const updateBranchQuery = useCallback(
		(field: BranchCompareField, query: string) => {
			updateBranchCompareModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				const filtered = filterComparableRefs(current.availableRefs, query);
				const currentRef =
					field === "source" ? current.sourceRef : current.destinationRef;
				const nextRef =
					Option.isSome(currentRef) && filtered.includes(currentRef.value)
						? currentRef
						: Option.fromNullable(filtered[0]);
				const nextIndex = Option.match(nextRef, {
					onNone: () => 0,
					onSome: (refName) => Math.max(filtered.indexOf(refName), 0),
				});
				return field === "source"
					? {
							...current,
							sourceQuery: query,
							sourceRef: nextRef,
							selectedSourceIndex: nextIndex,
							error: Option.none(),
						}
					: {
							...current,
							destinationQuery: query,
							destinationRef: nextRef,
							selectedDestinationIndex: nextIndex,
							error: Option.none(),
						};
			});
		},
		[filterComparableRefs, updateBranchCompareModal],
	);

	const onBranchSourceQueryChange = useCallback(
		(value: string) => {
			updateBranchQuery("source", value);
		},
		[updateBranchQuery],
	);

	const onBranchDestinationQueryChange = useCallback(
		(value: string) => {
			updateBranchQuery("destination", value);
		},
		[updateBranchQuery],
	);

	const selectBranchRef = useCallback(
		(refName: string) => {
			updateBranchCompareModal((current) => {
				if (!current.isOpen) {
					return current;
				}
				const activeField = current.activeField;
				const activeQuery =
					activeField === "source"
						? current.sourceQuery
						: current.destinationQuery;
				const filtered = filterComparableRefs(current.availableRefs, activeQuery);
				const nextIndex = Math.max(filtered.indexOf(refName), 0);

				return activeField === "source"
					? {
							...current,
							sourceRef: Option.some(refName),
							sourceQuery: refName,
							selectedSourceIndex: nextIndex,
							error: Option.none(),
						}
					: {
							...current,
							destinationRef: Option.some(refName),
							destinationQuery: refName,
							selectedDestinationIndex: nextIndex,
							error: Option.none(),
						};
			});
		},
		[filterComparableRefs, updateBranchCompareModal],
	);

	const moveBranchSelection = useCallback(
		(direction: 1 | -1) => {
			updateBranchCompareModal((current) => {
				if (!current.isOpen || current.loading) {
					return current;
				}
				const activeField = current.activeField;
				const activeQuery =
					activeField === "source"
						? current.sourceQuery
						: current.destinationQuery;
				const filtered = filterComparableRefs(current.availableRefs, activeQuery);
				if (filtered.length === 0) {
					return current;
				}
				const selectedIndex =
					activeField === "source"
						? current.selectedSourceIndex
						: current.selectedDestinationIndex;
				const baseIndex = Math.min(
					Math.max(selectedIndex, 0),
					filtered.length - 1,
				);
				const nextIndex = (baseIndex + direction + filtered.length) % filtered.length;
				const nextRef = filtered[nextIndex];
				if (!nextRef) {
					return current;
				}

				return activeField === "source"
					? {
							...current,
							sourceRef: Option.some(nextRef),
							selectedSourceIndex: nextIndex,
							error: Option.none(),
						}
					: {
							...current,
							destinationRef: Option.some(nextRef),
							selectedDestinationIndex: nextIndex,
							error: Option.none(),
						};
			});
		},
		[filterComparableRefs, updateBranchCompareModal],
	);

	const switchBranchField = useCallback(() => {
		updateBranchCompareModal((current) => {
			if (!current.isOpen) {
				return current;
			}
			return {
				...current,
				activeField:
					current.activeField === "source" ? "destination" : "source",
			};
		});
	}, [updateBranchCompareModal]);

	const confirmBranchCompareModal = useCallback(() => {
		if (!branchCompareModal.isOpen || branchCompareModal.loading) {
			return;
		}

		if (
			Option.isNone(branchCompareModal.sourceRef) ||
			Option.isNone(branchCompareModal.destinationRef)
		) {
			updateBranchCompareModal((current) =>
				current.isOpen
					? {
							...current,
							error: Option.some("Select both source and destination refs."),
						}
					: current,
			);
			return;
		}

		const sourceRef = branchCompareModal.sourceRef.value;
		const destinationRef = branchCompareModal.destinationRef.value;
		if (sourceRef === destinationRef) {
			updateBranchCompareModal((current) =>
				current.isOpen
					? {
							...current,
							error: Option.some(
								"Source and destination refs must be different.",
							),
						}
					: current,
			);
			return;
		}

		updateReviewMode(() => ({
			_tag: "branch-compare",
			selection: {
				sourceRef,
				destinationRef,
			},
		}));
		updateBranchCompareModal(() => ({ isOpen: false }));
		clearUiError();
		void refreshFiles(true);
	}, [
		branchCompareModal,
		clearUiError,
		refreshFiles,
		updateBranchCompareModal,
		updateReviewMode,
	]);

	const resetReviewMode = useCallback(() => {
		if (reviewMode._tag === "working-tree") {
			return;
		}
		updateReviewMode(() => ({ _tag: "working-tree" }));
		clearUiError();
		void refreshFiles(true);
	}, [clearUiError, refreshFiles, reviewMode._tag, updateReviewMode]);

	const syncRemote = useCallback(
		(direction: "pull" | "push") => {
			if (remoteSync._tag === "running") {
				return;
			}

			updateRemoteSync(() => ({
				_tag: "running",
				direction,
			}));
			clearUiError();

			void Effect.runPromise(
				pipe(
					direction === "push" ? pushToRemote() : pullFromRemote(),
					Effect.match({
						onFailure: (error) => {
							setUiError(renderRepoActionError(error));
						},
						onSuccess: () => {
							clearUiError();
							void refreshFiles(false);
						},
					}),
					Effect.ensuring(
						Effect.sync(() => {
							updateRemoteSync(() => ({ _tag: "idle" }));
						}),
					),
				),
			);
		},
		[
			clearUiError,
			refreshFiles,
			remoteSync,
			renderRepoActionError,
			setUiError,
			updateRemoteSync,
		],
	);

	const toggleSelectedFileStage = useCallback(
		(file: FileEntry) => {
			if (reviewMode._tag !== "working-tree") {
				return;
			}
			runAction(toggleFileStage(file), renderRepoActionError);
		},
		[renderRepoActionError, reviewMode._tag, runAction],
	);

	const initializeGitRepository = useCallback(() => {
		if (!canInitializeGitRepo) {
			return;
		}
		runAction(initGitRepository(), renderRepoActionError, {
			refreshOnFailure: true,
		});
	}, [canInitializeGitRepo, renderRepoActionError, runAction]);

	const onKeyboardIntent = useCallback(
		(intent: AppKeyboardIntent) =>
			Match.value(intent).pipe(
				Match.tagsExhaustive({
					DestroyRenderer: () => {
						renderer.destroy();
					},
					ToggleSidebar: () => {
						toggleSidebar();
					},
					ToggleDiffViewMode: () => {
						toggleDiffViewMode();
					},
					CloseCommitModal: () => {
						closeCommitModal();
					},
					OpenCommitModal: () => {
						openCommitModal();
					},
					CloseDiscardModal: () => {
						closeDiscardModal();
					},
					OpenDiscardModal: (typedIntent) => {
						openDiscardModal(typedIntent.file);
					},
					ConfirmDiscardModal: () => {
						confirmDiscardModal();
					},
					CloseHelpModal: () => {
						closeHelpModal();
					},
					OpenHelpModal: () => {
						openHelpModal();
					},
					InitGitRepository: () => {
						initializeGitRepository();
					},
					OpenThemeModal: () => {
						openThemeModal();
					},
					OpenBranchCompareModal: () => {
						openBranchCompareModal();
					},
					CloseThemeModal: () => {
						closeThemeModal();
					},
					CloseBranchCompareModal: () => {
						closeBranchCompareModal();
					},
					ConfirmThemeModal: () => {
						confirmThemeModal();
					},
					ConfirmBranchCompareModal: () => {
						confirmBranchCompareModal();
					},
					MoveThemeSelection: (typedIntent) => {
						moveThemeSelection(typedIntent.direction);
					},
					MoveBranchSelection: (typedIntent) => {
						moveBranchSelection(typedIntent.direction);
					},
					SwitchBranchModalField: () => {
						switchBranchField();
					},
					SyncRemote: (typedIntent) => {
						syncRemote(typedIntent.direction);
					},
					ResetReviewMode: () => {
						resetReviewMode();
					},
					ScrollDiffHalfPage: (typedIntent) => {
						const step = Math.max(6, Math.floor(renderer.height * 0.45));
						diffScrollRef.current?.scrollBy({
							x: 0,
							y: typedIntent.direction === "up" ? -step : step,
						});
					},
					OpenSelectedFile: (typedIntent) => {
						openSelectedFile(typedIntent.filePath);
					},
					ToggleSelectedFileStage: (typedIntent) => {
						toggleSelectedFileStage(typedIntent.file);
					},
					SelectVisiblePath: (typedIntent) => {
						selectFilePath(typedIntent.path);
					},
				}),
			),
		[
			closeCommitModal,
			closeDiscardModal,
			closeHelpModal,
			closeThemeModal,
			confirmDiscardModal,
			confirmThemeModal,
			initializeGitRepository,
			moveThemeSelection,
			moveBranchSelection,
			openCommitModal,
			openBranchCompareModal,
			openDiscardModal,
			openHelpModal,
			openThemeModal,
			openSelectedFile,
			renderer,
			diffScrollRef,
			resetReviewMode,
			selectFilePath,
			switchBranchField,
			syncRemote,
			toggleDiffViewMode,
			toggleSidebar,
			toggleSelectedFileStage,
			closeBranchCompareModal,
			confirmBranchCompareModal,
		],
	);

	return {
		onCommitMessageChange,
		onCommitSubmit,
		onCancelDiscardModal: closeDiscardModal,
		onConfirmDiscardModal: confirmDiscardModal,
		onBranchSourceQueryChange,
		onBranchDestinationQueryChange,
		onBranchSelectRef: selectBranchRef,
		onBranchActivateField: activateBranchField,
		onKeyboardIntent,
		onToggleDirectory: toggleCollapsedDirectory,
		onSelectFilePath: selectFilePath,
		onSelectThemeInModal: selectThemeInModal,
		onToggleSidebar: toggleSidebar,
	};
}
