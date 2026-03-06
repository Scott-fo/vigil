import { useAtom } from "@effect-atom/atom-react";
import type { ScrollBoxRenderable } from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { Effect, Option, pipe } from "effect";
import { useEffect, useMemo, useRef, useState } from "react";
import { type RepoActionError } from "#data/git.ts";
import { buildDiffNavigationModel } from "#diff/navigation.ts";
import type { ThemeMode } from "#theme/theme.ts";
import type { AppProps } from "#tui/types.ts";
import { BlameView } from "#ui/components/blame-view.tsx";
import { BranchCompareModal } from "#ui/components/branch-compare-modal.tsx";
import { CommitSearchModal } from "#ui/components/commit-search-modal.tsx";
import { CommitModal } from "#ui/components/commit-modal.tsx";
import { DiscardModal } from "#ui/components/discard-modal.tsx";
import { HelpModal } from "#ui/components/help-modal.tsx";
import { RemoteSyncStatus } from "#ui/components/remote-sync-status.tsx";
import { Reviewer } from "#ui/components/reviewer.tsx";
import { Snackbar } from "#ui/components/snackbar.tsx";
import { Splash } from "#ui/components/splash.tsx";
import { ThemeModal } from "#ui/components/theme-modal.tsx";
import { useAppSelectors } from "#ui/hooks/use-app-selectors.ts";
import { useBlameView } from "#ui/hooks/use-blame-view.ts";
import { useDaemonSession } from "#ui/hooks/use-daemon-session.ts";
import { useDaemonWatch } from "#ui/hooks/use-daemon-watch.ts";
import { useDiffPreviewState } from "#ui/hooks/use-diff-preview-state.ts";
import { useFileRefresh } from "#ui/hooks/use-file-refresh.ts";
import { useNotifications } from "#ui/hooks/use-notifications.ts";
import { usePaneNavigationState } from "#ui/hooks/use-pane-navigation-state.ts";
import { useRepoActions } from "#ui/hooks/use-repo-actions.ts";
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

export function App(props: AppProps) {
	const renderer = useRenderer();
	const [themeName, setThemeName] = useState(props.initialThemeName);
	const [themeSearchQuery, setThemeSearchQuery] = useState("");
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
	const [refreshInstructionVersion, setRefreshInstructionVersion] = useState(0);
	const diffScrollRef = useRef<ScrollBoxRenderable | null>(null);

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
		updateFileView: setFileView,
		updateUiStatus: setUiStatus,
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

	const {
		daemonSnackbarNotice,
		notifyDaemonDisconnected,
		notifyDaemonReconnected,
		onCopySelection,
		snackbarTop,
		transientSnackbarNotice,
		transientSnackbarTop,
	} = useNotifications({
		renderer,
		hasRemoteSyncRunning: remoteSync._tag === "running",
	});

	useDaemonSession({
		daemonConnection: props.daemonConnection,
		enabled: true,
		onDisconnect: notifyDaemonDisconnected,
		onReconnect: notifyDaemonReconnected,
	});

	useDaemonWatch({
		daemonConnection: props.daemonConnection,
		repoPath: watchRepoPath,
		enabled: isWorkingTreeMode,
		onRefreshInstruction,
	});

	useEffect(() => {
		if (!isThemeModalOpen) {
			return;
		}
		setThemeSearchQuery("");
	}, [isThemeModalOpen]);

	useEffect(() => {
		setFileView((current) => {
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
	}, [setFileView, visibleFilePaths]);

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
	const {
		activePane,
		selectedDiffLineIndex,
		setActivePane,
		setSelectedDiffLineIndex,
	} = usePaneNavigationState({
		selectedFilePath: selectedFile?.path,
		diffLineCount: diffNavigationModel.lines.length,
	});

	const diffLineCount = diffNavigationModel.lines.length;
	const selectedDiffLine =
		diffNavigationModel.lines[selectedDiffLineIndex] ?? null;
	const selectedDiffLineNumber =
		selectedDiffLine?.newLine ?? selectedDiffLine?.oldLine ?? null;
	const selectedDiffFilePath = selectedFile?.path ?? null;
	const {
		blameView,
		canOpenCommitCompare,
		close: closeBlameView,
		openCommitCompare: openBlameCommitCompare,
		scroll: scrollBlameView,
		scrollRef: blameScrollRef,
	} = useBlameView({
		initialTarget: props.initialBlameTarget,
		refreshFiles,
		renderRepoActionError: formatRepoActionError,
		updateReviewMode: setReviewMode,
		updateUiStatus: setUiStatus,
	});

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
		updateFileView: setFileView,
		updateUiStatus: setUiStatus,
		updateCommitModal: setCommitModal,
		updateCommitSearchModal: setCommitSearchModal,
		updateHelpModal: setHelpModal,
		updateThemeModal: setThemeModal,
		updateBranchCompareModal: setBranchCompareModal,
		remoteSync,
		updateRemoteSync: setRemoteSync,
		updateReviewMode: setReviewMode,
		closeBlameView,
		openBlameCommitCompare,
		scrollBlameView,
		updateDiscardModal: setDiscardModal,
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
		canOpenBlameCommitCompare: canOpenCommitCompare,
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
