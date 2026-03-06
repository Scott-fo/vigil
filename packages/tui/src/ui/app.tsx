import { useAtom } from "@effect-atom/atom-react";
import type { ScrollBoxRenderable } from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { Effect, Option, pipe } from "effect";
import { useMemo, useRef, useState } from "react";
import { type RepoActionError } from "#data/git.ts";
import { buildDiffNavigationModel } from "#diff/navigation.ts";
import type { ThemeMode } from "#theme/theme.ts";
import type { AppProps } from "#tui/types.ts";
import { AppEffects } from "#ui/components/app-effects.tsx";
import { GlobalOverlays } from "#ui/components/global-overlays.tsx";
import { Layout } from "#ui/components/layout.tsx";
import { useBlameView } from "#ui/hooks/use-blame-view.ts";
import { useBranchCompareView } from "#ui/hooks/use-branch-compare-view.ts";
import { useCommitSearchView } from "#ui/hooks/use-commit-search-view.ts";
import { useDiffPreviewState } from "#ui/hooks/use-diff-preview-state.ts";
import { useFileRefresh } from "#ui/hooks/use-file-refresh.ts";
import { useModalView } from "#ui/hooks/use-modal-view.ts";
import { useNotifications } from "#ui/hooks/use-notifications.ts";
import { usePaneNavigationState } from "#ui/hooks/use-pane-navigation-state.ts";
import { useReviewFileView } from "#ui/hooks/use-review-file-view.ts";
import { useReviewStatusView } from "#ui/hooks/use-review-status-view.ts";
import { useRepoActions } from "#ui/hooks/use-repo-actions.ts";
import { useThemeView } from "#ui/hooks/use-theme-view.ts";
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
		diffViewMode,
		files,
		loading,
		selectedFile,
		selectedVisibleIndex,
		sidebarItems,
		sidebarOpen,
		stagedFileCount,
		visibleFilePaths,
	} = useReviewFileView({
		fileView,
	});

	const { canInitializeGitRepo, reviewModeLabel } = useReviewStatusView({
		reviewMode,
		uiStatus,
	});

	const {
		filteredThemeNames,
		modalBackdropColor,
		selectedThemeName,
		theme,
		themeBundle,
	} = useThemeView({
		themeCatalog: props.themeCatalog,
		themeModal,
		themeMode,
		themeName,
		themeSearchQuery,
	});

	const {
		discardModalFile,
		isAnyModalOpen,
		isCommitModalOpen,
		isCommitSearchModalOpen,
		isDiscardModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		isBranchCompareModalOpen,
	} = useModalView({
		branchCompareModal,
		commitModal,
		commitSearchModal,
		discardModal,
		helpModal,
		themeModal,
	});

	const {
		branchActiveField,
		branchDestinationQuery,
		branchDestinationRef,
		branchFilteredRefs,
		branchModalError,
		branchModalLoading,
		branchSelectedActiveRef,
		branchSourceQuery,
		branchSourceRef,
	} = useBranchCompareView({
		branchCompareModal,
	});

	const commitMessage = commitModal.isOpen ? commitModal.message : "";
	const commitError = commitModal.isOpen ? commitModal.error : Option.none();

	const {
		commitFilteredCommits,
		commitSearchModalError,
		commitSearchModalLoading,
		commitSearchQuery,
		commitSelectedCommitHash,
		commitSelectedIndex,
	} = useCommitSearchView({
		commitSearchModal,
	});

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

	const reviewerProps = {
		theme,
		syntaxStyle: themeBundle.syntaxStyle,
		reviewModeLabel,
		files,
		sidebarItems,
		selectedFile,
		selectedFileDiff,
		selectedFileDiffNote,
		selectedFileDiffLoading,
		selectedDiffLineIndex,
		diffNavigationLines: diffNavigationModel.lines,
		loading,
		diffViewMode,
		error: uiStatus.error,
		isCommitModalOpen: isAnyModalOpen || blameView.isOpen,
		diffScrollRef,
		onToggleDirectory,
		onSelectFilePath,
		sidebarOpen,
		activePane,
		onToggleSidebar,
		onCopySelection,
	};

	return (
		<box
			flexDirection="column"
			flexGrow={1}
			padding={1}
			backgroundColor={theme.background}
		>
			<AppEffects
				activePane={activePane}
				canInitializeGitRepo={canInitializeGitRepo}
				canOpenBlameCommitCompare={canOpenCommitCompare}
				daemonConnection={props.daemonConnection}
				enabledWatch={isWorkingTreeMode}
				filteredThemeNames={filteredThemeNames}
				isBlameViewOpen={blameView.isOpen}
				isBranchCompareModalOpen={isBranchCompareModalOpen}
				isCommitModalOpen={isCommitModalOpen}
				isCommitSearchModalOpen={isCommitSearchModalOpen}
				isDiscardModalOpen={isDiscardModalOpen}
				isHelpModalOpen={isHelpModalOpen}
				isReadOnlyReviewMode={!isWorkingTreeMode}
				isThemeModalOpen={isThemeModalOpen}
				notifyDaemonDisconnected={notifyDaemonDisconnected}
				notifyDaemonReconnected={notifyDaemonReconnected}
				onIntent={onKeyboardIntent}
				onRefreshInstruction={onRefreshInstruction}
				onSelectThemeInModal={onSelectThemeInModal}
				selectedDiffFilePath={selectedDiffFilePath}
				selectedDiffLineNumber={selectedDiffLineNumber}
				selectedFile={selectedFile}
				selectedThemeName={selectedThemeName}
				selectedVisibleIndex={selectedVisibleIndex}
				setThemeSearchQuery={setThemeSearchQuery}
				stagedFileCount={stagedFileCount}
				updateFileView={setFileView}
				visibleFilePaths={visibleFilePaths}
			/>
			<Layout reviewerProps={reviewerProps} theme={theme} uiStatus={uiStatus} />
			<GlobalOverlays
				theme={theme}
				modalBackdropColor={modalBackdropColor}
				isCommitModalOpen={isCommitModalOpen}
				commitMessage={commitMessage}
				commitError={commitError}
				onCommitMessageChange={onCommitMessageChange}
				onCommitSubmit={onCommitSubmit}
				isDiscardModalOpen={isDiscardModalOpen}
				discardModalFile={discardModalFile}
				onCancelDiscardModal={onCancelDiscardModal}
				onConfirmDiscardModal={onConfirmDiscardModal}
				isHelpModalOpen={isHelpModalOpen}
				isThemeModalOpen={isThemeModalOpen}
				themeNames={filteredThemeNames}
				selectedThemeName={selectedThemeName}
				themeSearchQuery={themeSearchQuery}
				onSearchQueryChange={setThemeSearchQuery}
				onSelectTheme={onSelectThemeInModal}
				isBranchCompareModalOpen={isBranchCompareModalOpen}
				branchSourceQuery={branchSourceQuery}
				branchDestinationQuery={branchDestinationQuery}
				branchSourceRef={branchSourceRef}
				branchDestinationRef={branchDestinationRef}
				branchActiveField={branchActiveField}
				branchFilteredRefs={branchFilteredRefs}
				branchSelectedActiveRef={branchSelectedActiveRef}
				branchModalLoading={branchModalLoading}
				branchModalError={branchModalError}
				onBranchSourceQueryChange={onBranchSourceQueryChange}
				onBranchDestinationQueryChange={onBranchDestinationQueryChange}
				onBranchSelectRef={onBranchSelectRef}
				onBranchActivateField={onBranchActivateField}
				isCommitSearchModalOpen={isCommitSearchModalOpen}
				commitSearchQuery={commitSearchQuery}
				commitSearchCommits={commitFilteredCommits}
				commitSelectedCommitHash={commitSelectedCommitHash}
				commitSelectedIndex={commitSelectedIndex}
				commitSearchModalLoading={commitSearchModalLoading}
				commitSearchModalError={commitSearchModalError}
				onCommitSearchQueryChange={onCommitSearchQueryChange}
				onCommitSearchSelectCommit={onCommitSearchSelectCommit}
				isBlameViewOpen={blameView.isOpen}
				blameTarget={blameView.target}
				blameLoading={blameView.loading}
				blameDetails={blameView.details}
				blameError={blameView.error}
				blameScrollRef={blameScrollRef}
				remoteSync={remoteSync}
				daemonSnackbarNotice={daemonSnackbarNotice}
				transientSnackbarNotice={transientSnackbarNotice}
				snackbarTop={snackbarTop}
				transientSnackbarTop={transientSnackbarTop}
			/>
		</box>
	);
}
