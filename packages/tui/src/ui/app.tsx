import { useAtom } from "@effect-atom/atom-react";
import type { ScrollBoxRenderable } from "@opentui/core";
import { useRenderer } from "@opentui/react";
import { Effect, Option, pipe } from "effect";
import { useMemo, useRef } from "react";
import type { RepoActionError } from "#data/git.ts";
import { buildDiffNavigationModel } from "#diff/navigation.ts";
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
import { useRepoActions } from "#ui/hooks/use-repo-actions.ts";
import { useReviewFileView } from "#ui/hooks/use-review-file-view.ts";
import { useReviewStatusView } from "#ui/hooks/use-review-status-view.ts";
import { useThemeState } from "#ui/hooks/use-theme-state.ts";
import {
	branchCompareModalAtom,
	commitModalAtom,
	commitSearchModalAtom,
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
		themeMode,
		themeName,
		themeSearchQuery,
		setThemeName,
		setThemeSearchQuery,
	} = useThemeState({
		themeMode: props.themeMode,
		themeName: props.themeName,
		themeCatalog: props.themeCatalog,
		themeModal,
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

	const { refreshFiles, onRefreshInstruction, refreshInstructionVersion } =
		useFileRefresh({
			updateFileView: setFileView,
			updateUiStatus: setUiStatus,
			renderRepoActionError: formatRepoActionError,
			reviewMode,
		});

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
			visibleFilePaths,
			selectedFile,
			selectedVisibleIndex,
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

	const keyboard = {
		isBlameViewOpen: blameView.isOpen,
		canOpenBlameCommitCompare: canOpenCommitCompare,
		isCommitModalOpen,
		isDiscardModalOpen,
		isCommitSearchModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		isBranchCompareModalOpen,
		isReadOnlyReviewMode: !isWorkingTreeMode,
		activePane,
		canInitializeGitRepo,
		stagedFileCount,
		visibleFilePaths,
		selectedVisibleIndex,
		selectedFile,
		selectedDiffFilePath,
		selectedDiffLineNumber,
		onIntent: onKeyboardIntent,
	};

	const appEffectsTheme = {
		filteredThemeNames,
		isThemeModalOpen,
		onSelectThemeInModal,
		selectedThemeName,
		setThemeSearchQuery,
	};

	const reviewerProps = {
		theme,
		themeKey: `${themeBundle.name}:${themeBundle.mode}`,
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

	const commitOverlay = {
		isOpen: isCommitModalOpen,
		commitMessage,
		commitError,
		onCommitMessageChange,
		onCommitSubmit,
	};

	const discardOverlay = {
		isOpen: isDiscardModalOpen,
		discardModalFile,
		onCancelDiscardModal,
		onConfirmDiscardModal,
	};

	const themeOverlay = {
		isOpen: isThemeModalOpen,
		themeNames: filteredThemeNames,
		selectedThemeName,
		themeSearchQuery,
		onSearchQueryChange: setThemeSearchQuery,
		onSelectTheme: onSelectThemeInModal,
	};

	const branchCompareOverlay = {
		isOpen: isBranchCompareModalOpen,
		branchSourceQuery,
		branchDestinationQuery,
		branchSourceRef,
		branchDestinationRef,
		branchActiveField,
		branchFilteredRefs,
		branchSelectedActiveRef,
		branchModalLoading,
		branchModalError,
		onBranchSourceQueryChange,
		onBranchDestinationQueryChange,
		onBranchSelectRef,
		onBranchActivateField,
	};

	const commitSearchOverlay = {
		isOpen: isCommitSearchModalOpen,
		commitSearchQuery,
		commitSearchCommits: commitFilteredCommits,
		commitSelectedCommitHash,
		commitSelectedIndex,
		commitSearchModalLoading,
		commitSearchModalError,
		onCommitSearchQueryChange,
		onCommitSearchSelectCommit,
	};

	const blameOverlay = {
		isOpen: blameView.isOpen,
		blameTarget: blameView.target,
		blameLoading: blameView.loading,
		blameDetails: blameView.details,
		blameError: blameView.error,
		blameScrollRef,
	};

	const notificationsOverlay = {
		remoteSync,
		daemonSnackbarNotice,
		transientSnackbarNotice,
		snackbarTop,
		transientSnackbarTop,
	};

	return (
		<box
			flexDirection="column"
			flexGrow={1}
			padding={1}
			backgroundColor={theme.background}
		>
			<AppEffects
				daemonConnection={props.daemonConnection}
				enabledWatch={isWorkingTreeMode}
				keyboard={keyboard}
				notifyDaemonDisconnected={notifyDaemonDisconnected}
				notifyDaemonReconnected={notifyDaemonReconnected}
				onRefreshInstruction={onRefreshInstruction}
				theme={appEffectsTheme}
				updateFileView={setFileView}
				visibleFilePaths={visibleFilePaths}
			/>
			<Layout reviewerProps={reviewerProps} theme={theme} uiStatus={uiStatus} />
			<GlobalOverlays
				theme={theme}
				modalBackdropColor={modalBackdropColor}
				commit={commitOverlay}
				discard={discardOverlay}
				isHelpModalOpen={isHelpModalOpen}
				themeModal={themeOverlay}
				branchCompare={branchCompareOverlay}
				commitSearch={commitSearchOverlay}
				blameView={blameOverlay}
				notifications={notificationsOverlay}
			/>
		</box>
	);
}
