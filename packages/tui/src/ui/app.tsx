import { useAtom } from "@effect-atom/atom-react";
import type { ScrollBoxRenderable } from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { Effect, Match, Option, pipe } from "effect";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { copyTextToClipboard } from "#data/clipboard.ts";
import {
	loadBlameCommitDetails,
	type BlameCommitDetails,
	type RepoActionError,
} from "#data/git.ts";
import { buildDiffNavigationModel } from "#diff/navigation.ts";
import type { ThemeMode } from "#theme/theme.ts";
import type { AppProps, BlameTarget } from "#tui/types.ts";
import { BlameView } from "#ui/components/blame-view.tsx";
import { BranchCompareModal } from "#ui/components/branch-compare-modal.tsx";
import { CommitSearchModal } from "#ui/components/commit-search-modal.tsx";
import { CommitModal } from "#ui/components/commit-modal.tsx";
import { DiscardModal } from "#ui/components/discard-modal.tsx";
import { HelpModal } from "#ui/components/help-modal.tsx";
import { RemoteSyncStatus } from "#ui/components/remote-sync-status.tsx";
import { Reviewer } from "#ui/components/reviewer.tsx";
import { Snackbar, type SnackbarNotice } from "#ui/components/snackbar.tsx";
import { Splash } from "#ui/components/splash.tsx";
import { ThemeModal } from "#ui/components/theme-modal.tsx";
import { useAppSelectors } from "#ui/hooks/use-app-selectors.ts";
import { useDaemonSession } from "#ui/hooks/use-daemon-session.ts";
import { useDaemonWatch } from "#ui/hooks/use-daemon-watch.ts";
import { useDiffPreviewState } from "#ui/hooks/use-diff-preview-state.ts";
import { useFileRefresh } from "#ui/hooks/use-file-refresh.ts";
import { useRepoActions } from "#ui/hooks/use-repo-actions.ts";
import type { FocusedPane } from "#ui/inputs.ts";
import { useAppKeyboardInput } from "#ui/inputs.ts";
import {
	branchCompareModalAtom,
	commitSearchModalAtom,
	commitModalAtom,
	discardModalAtom,
	fileViewStateAtom,
	helpModalAtom,
	isWorkingTreeReviewMode,
	remoteSyncAtom,
	reviewModeAtom,
	themeModalAtom,
	type UpdateBranchCompareModal,
	type UpdateCommitSearchModal,
	type UpdateCommitModal,
	type UpdateDiscardModal,
	type UpdateFileViewState,
	type UpdateHelpModal,
	type UpdateRemoteSyncState,
	type UpdateReviewMode,
	type UpdateThemeModal,
	type UpdateUiStatus,
	uiStatusAtom,
} from "#ui/state.ts";

function formatRepoActionError(error: RepoActionError): string {
	return pipe(
		Effect.fail(error),
		Effect.catchTags({
			CommitMessageRequiredError: (typedError) =>
				Effect.succeed(typedError.message),
			GitCommandError: (typedError) =>
				Effect.succeed(
					typedError.stderr.trim() ||
						typedError.stdout.trim() ||
						typedError.fallbackMessage,
				),
		}),
		Effect.runSync,
	);
}

interface BlameViewState {
	readonly isOpen: boolean;
	readonly target: BlameTarget | null;
	readonly loading: boolean;
	readonly details: BlameCommitDetails | null;
	readonly error: string | null;
}

function createBlameViewState(
	initialTarget: Option.Option<BlameTarget>,
): BlameViewState {
	return Option.match(initialTarget, {
		onNone: () => ({
			isOpen: false,
			target: null,
			loading: false,
			details: null,
			error: null,
		}),
		onSome: (target) => ({
			isOpen: true,
			target,
			loading: true,
			details: null,
			error: null,
		}),
	});
}

export function App(props: AppProps) {
	const renderer = useRenderer();
	const [themeName, setThemeName] = useState(props.initialThemeName);
	const [themeSearchQuery, setThemeSearchQuery] = useState("");
	const [activePane, setActivePane] = useState<FocusedPane>("sidebar");
	const [selectedDiffLineIndex, setSelectedDiffLineIndex] = useState(0);
	const [themeMode] = useState<ThemeMode>(props.initialThemeMode);
	const [fileView, setFileView] = useAtom(fileViewStateAtom);
	const [uiStatus, setUiStatus] = useAtom(uiStatusAtom);
	const [commitModal, setCommitModal] = useAtom(commitModalAtom);
	const [discardModal, setDiscardModal] = useAtom(discardModalAtom);
	const [commitSearchModal, setCommitSearchModal] = useAtom(
		commitSearchModalAtom,
	);
	const [helpModal, setHelpModal] = useAtom(helpModalAtom);
	const [themeModal, setThemeModal] = useAtom(themeModalAtom);
	const [branchCompareModal, setBranchCompareModal] = useAtom(
		branchCompareModalAtom,
	);
	const [remoteSync, setRemoteSync] = useAtom(remoteSyncAtom);
	const [reviewMode, setReviewMode] = useAtom(reviewModeAtom);
	const [blameView, setBlameView] = useState<BlameViewState>(() =>
		createBlameViewState(props.initialBlameTarget),
	);
	const [refreshInstructionVersion, setRefreshInstructionVersion] = useState(0);
	const [daemonSnackbarNotice, setDaemonSnackbarNotice] = useState<
		Option.Option<SnackbarNotice>
	>(Option.none());
	const [transientSnackbarNotice, setTransientSnackbarNotice] = useState<
		Option.Option<SnackbarNotice>
	>(Option.none());
	const diffScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const blameScrollRef = useRef<ScrollBoxRenderable | null>(null);
	const snackbarTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	const updateFileView = useCallback<UpdateFileViewState>(
		(update) => {
			setFileView(update);
		},
		[setFileView],
	);

	const updateUiStatus = useCallback<UpdateUiStatus>(
		(update) => {
			setUiStatus(update);
		},
		[setUiStatus],
	);

	const updateCommitModal = useCallback<UpdateCommitModal>(
		(update) => {
			setCommitModal(update);
		},
		[setCommitModal],
	);

	const updateCommitSearchModal = useCallback<UpdateCommitSearchModal>(
		(update) => {
			setCommitSearchModal(update);
		},
		[setCommitSearchModal],
	);

	const updateHelpModal = useCallback<UpdateHelpModal>(
		(update) => {
			setHelpModal(update);
		},
		[setHelpModal],
	);

	const updateThemeModal = useCallback<UpdateThemeModal>(
		(update) => {
			setThemeModal(update);
		},
		[setThemeModal],
	);

	const updateBranchCompareModal = useCallback<UpdateBranchCompareModal>(
		(update) => {
			setBranchCompareModal(update);
		},
		[setBranchCompareModal],
	);

	const updateRemoteSync = useCallback<UpdateRemoteSyncState>(
		(update) => {
			setRemoteSync(update);
		},
		[setRemoteSync],
	);

	const updateReviewMode = useCallback<UpdateReviewMode>(
		(update) => {
			setReviewMode(update);
		},
		[setReviewMode],
	);

	const updateDiscardModal = useCallback<UpdateDiscardModal>(
		(update) => {
			setDiscardModal(update);
		},
		[setDiscardModal],
	);

	const {
		files,
		sidebarOpen,
		diffViewMode,
		loading,
		themeBundle,
		theme,
		modalBackdropColor,
		isCommitModalOpen,
		isDiscardModalOpen,
		isCommitSearchModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		isBranchCompareModalOpen,
		isAnyModalOpen,
		discardModalFile,
		selectedThemeName,
		branchSourceQuery,
		branchDestinationQuery,
		branchSourceRef,
		branchDestinationRef,
		branchActiveField,
		branchFilteredRefs,
		branchSelectedActiveRef,
		branchModalLoading,
		branchModalError,
		filteredThemeNames,
		commitMessage,
		commitError,
		commitSearchQuery,
		commitFilteredCommits,
		commitSelectedCommitHash,
		commitSelectedIndex,
		commitSearchModalLoading,
		commitSearchModalError,
		canInitializeGitRepo,
		reviewModeLabel,
		selectedFile,
		sidebarItems,
		visibleFilePaths,
		selectedVisibleIndex,
		stagedFileCount,
	} = useAppSelectors({
		fileView,
		uiStatus,
		commitModal,
		discardModal,
		commitSearchModal,
		helpModal,
		themeModal,
		branchCompareModal,
		reviewMode,
		themeCatalog: props.themeCatalog,
		themeName,
		themeMode,
		themeSearchQuery,
	});

	const watchRepoPath = useMemo(() => process.cwd(), []);
	const isWorkingTreeMode = isWorkingTreeReviewMode(reviewMode);

	const { refreshFiles, refreshFilesEffect } = useFileRefresh({
		updateFileView,
		updateUiStatus,
		renderRepoActionError: formatRepoActionError,
		reviewMode,
	});

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

	const showSnackbar = useCallback((notice: SnackbarNotice) => {
		if (snackbarTimeoutRef.current) {
			clearTimeout(snackbarTimeoutRef.current);
		}
		setTransientSnackbarNotice(Option.some(notice));
		const timeoutHandle = setTimeout(() => {
			setTransientSnackbarNotice(Option.none());
		}, 2000);
		timeoutHandle.unref?.();
		snackbarTimeoutRef.current = timeoutHandle;
	}, []);

	const showDaemonDisconnected = useCallback((message: string) => {
		setDaemonSnackbarNotice(
			Option.some({
				message,
				variant: "error",
			}),
		);
	}, []);

	const clearDaemonDisconnected = useCallback(() => {
		setDaemonSnackbarNotice(Option.none());
		showSnackbar({
			message: "Reconnected to background daemon",
			variant: "info",
		});
	}, [showSnackbar]);

	useDaemonSession({
		daemonConnection: props.daemonConnection,
		enabled: true,
		onDisconnect: showDaemonDisconnected,
		onReconnect: clearDaemonDisconnected,
	});

	useDaemonWatch({
		daemonConnection: props.daemonConnection,
		repoPath: watchRepoPath,
		enabled: isWorkingTreeMode,
		onRefreshInstruction,
	});

	const copyAndNotify = useCallback(
		(text: string) => {
			if (text.length === 0) {
				return;
			}

			void Effect.runPromise(
				pipe(
					copyTextToClipboard(renderer, text),
					Effect.match({
						onFailure: (error) => {
							showSnackbar({
								message: Match.value(error).pipe(
									Match.tag(
										"NativeClipboardCopyError",
										(typedError) => typedError.message,
									),
									Match.tag(
										"ClipboardUnavailableError",
										(typedError) => typedError.message,
									),
									Match.exhaustive,
								),
								variant: "error",
							});
						},
						onSuccess: () => {
							showSnackbar({
								message: "Text copied to clipboard",
								variant: "info",
							});
						},
					}),
				),
			);
		},
		[renderer, showSnackbar],
	);

	const onCopySelection = useCallback(() => {
		const text = renderer.getSelection()?.getSelectedText();
		if (!text) {
			return;
		}
		copyAndNotify(text);
		renderer.clearSelection();
	}, [copyAndNotify, renderer]);

	useEffect(() => {
		if (!isThemeModalOpen) {
			return;
		}
		setThemeSearchQuery("");
	}, [isThemeModalOpen]);

	useEffect(() => {
		updateFileView((current) => {
			if (visibleFilePaths.length === 0) {
				return Option.isNone(current.selectedPath)
					? current
					: { ...current, selectedPath: Option.none() };
			}
			if (
				Option.isSome(current.selectedPath) &&
				visibleFilePaths.includes(current.selectedPath.value)
			) {
				return current;
			}
			return {
				...current,
				selectedPath: Option.fromNullable(visibleFilePaths[0]),
			};
		});
	}, [updateFileView, visibleFilePaths]);

	useEffect(
		() => () => {
			if (snackbarTimeoutRef.current) {
				clearTimeout(snackbarTimeoutRef.current);
			}
		},
		[],
	);

	useEffect(() => {
		renderer.console.onCopySelection = (text: string) => {
			if (!text) {
				return;
			}
			copyAndNotify(text);
			renderer.clearSelection();
		};
		return () => {
			renderer.console.onCopySelection = undefined;
		};
	}, [copyAndNotify, renderer]);

	const { selectedFileDiff, selectedFileDiffNote, selectedFileDiffLoading } =
		useDiffPreviewState({
			files,
			selectedFile,
			reviewMode,
			externalRefreshVersion: refreshInstructionVersion,
		});

	const diffNavigationModel = useMemo(
		() => buildDiffNavigationModel(selectedFileDiff),
		[selectedFileDiff],
	);
	const snackbarTop = remoteSync._tag === "running" ? 4 : 1;
	const transientSnackbarTop = Option.isSome(daemonSnackbarNotice)
		? snackbarTop + 4
		: snackbarTop;

	const diffLineCount = diffNavigationModel.lines.length;
	const selectedDiffLine =
		diffNavigationModel.lines[selectedDiffLineIndex] ?? null;
	const selectedDiffLineNumber =
		selectedDiffLine?.newLine ?? selectedDiffLine?.oldLine ?? null;
	const selectedDiffFilePath = selectedFile?.path ?? null;
	const canOpenBlameCommitCompare =
		blameView.isOpen &&
		blameView.details !== null &&
		Option.isSome(blameView.details.compareSelection);

	useEffect(() => {
		setSelectedDiffLineIndex(0);
	}, [selectedFile?.path]);

	useEffect(() => {
		setSelectedDiffLineIndex((current) => {
			if (diffLineCount === 0) {
				return 0;
			}
			return Math.min(current, diffLineCount - 1);
		});
	}, [diffLineCount]);

	useEffect(() => {
		if (!blameView.isOpen || !blameView.loading || !blameView.target) {
			return;
		}

		const target = blameView.target;
		let cancelled = false;

		void Effect.runPromise(
			pipe(
				loadBlameCommitDetails(target),
				Effect.match({
					onFailure: (error) => ({
						ok: false as const,
						error: formatRepoActionError(error),
					}),
					onSuccess: (details) => ({
						ok: true as const,
						details,
					}),
				}),
			),
		).then((result) => {
			if (cancelled) {
				return;
			}
			setBlameView((current) => {
				if (
					!current.isOpen ||
					!current.loading ||
					!current.target ||
					current.target.filePath !== target.filePath ||
					current.target.lineNumber !== target.lineNumber
				) {
					return current;
				}

				return result.ok
					? {
							...current,
							loading: false,
							details: result.details,
							error: null,
						}
					: {
							...current,
							loading: false,
							details: null,
							error: result.error,
						};
			});
		});

		return () => {
			cancelled = true;
		};
	}, [blameView.isOpen, blameView.loading, blameView.target]);

	const closeBlameView = useCallback(() => {
		setBlameView((current) =>
			current.isOpen ? { ...current, isOpen: false } : current,
		);
	}, []);

	const openBlameCommitCompare = useCallback(() => {
		if (!blameView.isOpen || !blameView.details) {
			return;
		}
		if (Option.isNone(blameView.details.compareSelection)) {
			setBlameView((current) =>
				current.isOpen
					? {
							...current,
							error: "No committed change is available for this line.",
						}
					: current,
			);
			return;
		}

		const selection = blameView.details.compareSelection.value;
		updateReviewMode(() => ({
			_tag: "commit-compare",
			selection,
		}));
		updateUiStatus((current) =>
			Option.isNone(current.error)
				? current
				: { ...current, error: Option.none() },
		);
		setBlameView((current) =>
			current.isOpen ? { ...current, isOpen: false } : current,
		);
		void refreshFiles(true);
	}, [blameView, refreshFiles, updateReviewMode, updateUiStatus]);

	const scrollBlameView = useCallback((direction: "up" | "down") => {
		const scroll = blameScrollRef.current;
		if (!scroll) {
			return;
		}
		scroll.scrollBy({
			x: 0,
			y: direction === "up" ? -3 : 3,
		});
	}, []);

	const {
		onCommitMessageChange,
		onCommitSubmit,
		onCancelDiscardModal,
		onConfirmDiscardModal,
		onBranchSourceQueryChange,
		onBranchDestinationQueryChange,
		onBranchSelectRef,
		onBranchActivateField,
		onCommitSearchQueryChange,
		onCommitSearchSelectCommit,
		onKeyboardIntent,
		onToggleDirectory,
		onSelectFilePath,
		onSelectThemeInModal,
		onToggleSidebar,
	} = useRepoActions({
		chooserFilePath: props.chooserFilePath,
		renderer,
		diffScrollRef,
		diffLineCount,
		themeName,
		themeMode,
		themeCatalog: props.themeCatalog,
		themeModalThemeNames: filteredThemeNames,
		setThemeName,
		stagedFileCount,
		sidebarOpen,
		activePane,
		setActivePane,
		setSelectedDiffLineIndex,
		commitModal,
		discardModal,
		commitSearchModal,
		themeModal,
		branchCompareModal,
		canInitializeGitRepo,
		reviewMode,
		updateFileView,
		updateUiStatus,
		updateCommitModal,
		updateCommitSearchModal,
		updateHelpModal,
		updateThemeModal,
		updateBranchCompareModal,
		remoteSync,
		updateRemoteSync,
		updateReviewMode,
		closeBlameView,
		openBlameCommitCompare,
		scrollBlameView,
		updateDiscardModal,
		refreshFiles,
		renderRepoActionError: formatRepoActionError,
	});

	useEffect(() => {
		if (!isThemeModalOpen || filteredThemeNames.length === 0) {
			return;
		}
		if (filteredThemeNames.includes(selectedThemeName)) {
			return;
		}
		const firstFilteredThemeName = filteredThemeNames[0];
		if (firstFilteredThemeName) {
			onSelectThemeInModal(firstFilteredThemeName);
		}
	}, [
		filteredThemeNames,
		isThemeModalOpen,
		onSelectThemeInModal,
		selectedThemeName,
	]);

	useAppKeyboardInput({
		isBlameViewOpen: blameView.isOpen,
		canOpenBlameCommitCompare,
		isCommitModalOpen,
		isDiscardModalOpen,
		isCommitSearchModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		isBranchCompareModalOpen,
		isReadOnlyReviewMode: !isWorkingTreeReviewMode(reviewMode),
		activePane,
		canInitializeGitRepo,
		stagedFileCount,
		visibleFilePaths,
		selectedVisibleIndex,
		selectedFile,
		selectedDiffFilePath,
		selectedDiffLineNumber,
		onIntent: onKeyboardIntent,
	});

	return (
		<box
			flexDirection="column"
			flexGrow={1}
			padding={1}
			backgroundColor={theme.background}
		>
			{uiStatus.showSplash ? (
				<Splash theme={theme} error={uiStatus.error} />
			) : (
				<Reviewer
					theme={theme}
					syntaxStyle={themeBundle.syntaxStyle}
					reviewModeLabel={reviewModeLabel}
					files={files}
					sidebarItems={sidebarItems}
					selectedFile={selectedFile}
					selectedFileDiff={selectedFileDiff}
					selectedFileDiffNote={selectedFileDiffNote}
					selectedFileDiffLoading={selectedFileDiffLoading}
					selectedDiffLineIndex={selectedDiffLineIndex}
					diffNavigationLines={diffNavigationModel.lines}
					loading={loading}
					diffViewMode={diffViewMode}
					error={uiStatus.error}
					isCommitModalOpen={isAnyModalOpen || blameView.isOpen}
					diffScrollRef={diffScrollRef}
					onToggleDirectory={onToggleDirectory}
					onSelectFilePath={onSelectFilePath}
					sidebarOpen={sidebarOpen}
					onToggleSidebar={onToggleSidebar}
					activePane={activePane}
					onCopySelection={onCopySelection}
				/>
			)}
			{isCommitModalOpen && (
				<CommitModal
					theme={theme}
					modalBackdropColor={modalBackdropColor}
					commitMessage={commitMessage}
					commitError={commitError}
					onCommitMessageChange={onCommitMessageChange}
					onCommitSubmit={onCommitSubmit}
				/>
			)}
			{isDiscardModalOpen && discardModalFile && (
				<DiscardModal
					theme={theme}
					modalBackdropColor={modalBackdropColor}
					file={discardModalFile}
					onCancel={onCancelDiscardModal}
					onConfirm={onConfirmDiscardModal}
				/>
			)}
			{isHelpModalOpen && (
				<HelpModal theme={theme} modalBackdropColor={modalBackdropColor} />
			)}
			{isThemeModalOpen && (
				<ThemeModal
					theme={theme}
					modalBackdropColor={modalBackdropColor}
					themes={filteredThemeNames}
					selectedThemeName={selectedThemeName}
					searchQuery={themeSearchQuery}
					onSearchQueryChange={setThemeSearchQuery}
					onSelectTheme={onSelectThemeInModal}
				/>
			)}
			{isBranchCompareModalOpen && (
				<BranchCompareModal
					theme={theme}
					modalBackdropColor={modalBackdropColor}
					sourceQuery={branchSourceQuery}
					destinationQuery={branchDestinationQuery}
					sourceRef={branchSourceRef}
					destinationRef={branchDestinationRef}
					activeField={branchActiveField}
					filteredRefs={branchFilteredRefs}
					selectedActiveRef={branchSelectedActiveRef}
					loading={branchModalLoading}
					error={branchModalError}
					onSourceQueryChange={onBranchSourceQueryChange}
					onDestinationQueryChange={onBranchDestinationQueryChange}
					onSelectRef={onBranchSelectRef}
					onActivateField={onBranchActivateField}
				/>
			)}
			{isCommitSearchModalOpen && (
				<CommitSearchModal
					theme={theme}
					modalBackdropColor={modalBackdropColor}
					query={commitSearchQuery}
					commits={commitFilteredCommits}
					selectedCommitHash={commitSelectedCommitHash}
					selectedIndex={commitSelectedIndex}
					loading={commitSearchModalLoading}
					error={commitSearchModalError}
					onQueryChange={onCommitSearchQueryChange}
					onSelectCommit={onCommitSearchSelectCommit}
				/>
			)}
			{blameView.isOpen && blameView.target && (
				<BlameView
					theme={theme}
					modalBackdropColor={modalBackdropColor}
					target={blameView.target}
					loading={blameView.loading}
					details={Option.fromNullable(blameView.details)}
					error={Option.fromNullable(blameView.error)}
					scrollRef={blameScrollRef}
				/>
			)}
			<RemoteSyncStatus theme={theme} state={remoteSync} />
			<Snackbar
				theme={theme}
				notice={daemonSnackbarNotice}
				top={snackbarTop}
			/>
			<Snackbar
				theme={theme}
				notice={transientSnackbarNotice}
				top={transientSnackbarTop}
			/>
		</box>
	);
}
