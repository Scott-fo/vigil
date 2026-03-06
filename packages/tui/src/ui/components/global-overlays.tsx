import type { RGBA, ScrollBoxRenderable } from "@opentui/core";
import { Option } from "effect";
import type { RefObject } from "react";
import type { BlameCommitDetails, CommitDiffSelection } from "#data/git.ts";
import type { ResolvedTheme } from "#theme/theme.ts";
import type { BlameTarget, FileEntry } from "#tui/types.ts";
import { BlameView } from "#ui/components/blame-view.tsx";
import { BranchCompareModal } from "#ui/components/branch-compare-modal.tsx";
import { CommitSearchModal } from "#ui/components/commit-search-modal.tsx";
import { CommitModal } from "#ui/components/commit-modal.tsx";
import { DiscardModal } from "#ui/components/discard-modal.tsx";
import { HelpModal } from "#ui/components/help-modal.tsx";
import { RemoteSyncStatus } from "#ui/components/remote-sync-status.tsx";
import { Snackbar, type SnackbarNotice } from "#ui/components/snackbar.tsx";
import { ThemeModal } from "#ui/components/theme-modal.tsx";
import type { BranchCompareField, RemoteSyncState } from "#ui/state.ts";

interface CommitOverlayProps {
	readonly isOpen: boolean;
	readonly commitMessage: string;
	readonly commitError: Option.Option<string>;
	readonly onCommitMessageChange: (value: string) => void;
	readonly onCommitSubmit: (payload: unknown) => void;
}

interface DiscardOverlayProps {
	readonly isOpen: boolean;
	readonly discardModalFile: FileEntry | null;
	readonly onCancelDiscardModal: () => void;
	readonly onConfirmDiscardModal: () => void;
}

interface ThemeOverlayProps {
	readonly isOpen: boolean;
	readonly themeNames: ReadonlyArray<string>;
	readonly selectedThemeName: string;
	readonly themeSearchQuery: string;
	readonly onSearchQueryChange: (value: string) => void;
	readonly onSelectTheme: (themeName: string) => void;
}

interface BranchCompareOverlayProps {
	readonly isOpen: boolean;
	readonly branchSourceQuery: string;
	readonly branchDestinationQuery: string;
	readonly branchSourceRef: Option.Option<string>;
	readonly branchDestinationRef: Option.Option<string>;
	readonly branchActiveField: BranchCompareField;
	readonly branchFilteredRefs: ReadonlyArray<string>;
	readonly branchSelectedActiveRef: Option.Option<string>;
	readonly branchModalLoading: boolean;
	readonly branchModalError: Option.Option<string>;
	readonly onBranchSourceQueryChange: (value: string) => void;
	readonly onBranchDestinationQueryChange: (value: string) => void;
	readonly onBranchSelectRef: (refName: string) => void;
	readonly onBranchActivateField: (field: BranchCompareField) => void;
}

interface CommitSearchOverlayProps {
	readonly isOpen: boolean;
	readonly commitSearchQuery: string;
	readonly commitSearchCommits: ReadonlyArray<CommitDiffSelection>;
	readonly commitSelectedCommitHash: Option.Option<string>;
	readonly commitSelectedIndex: number;
	readonly commitSearchModalLoading: boolean;
	readonly commitSearchModalError: Option.Option<string>;
	readonly onCommitSearchQueryChange: (query: string) => void;
	readonly onCommitSearchSelectCommit: (commitHash: string) => void;
}

interface BlameOverlayProps {
	readonly isOpen: boolean;
	readonly blameTarget: BlameTarget | null;
	readonly blameLoading: boolean;
	readonly blameDetails: BlameCommitDetails | null;
	readonly blameError: string | null;
	readonly blameScrollRef: RefObject<ScrollBoxRenderable | null>;
}

interface NotificationsOverlayProps {
	readonly remoteSync: RemoteSyncState;
	readonly daemonSnackbarNotice: Option.Option<SnackbarNotice>;
	readonly transientSnackbarNotice: Option.Option<SnackbarNotice>;
	readonly snackbarTop: number;
	readonly transientSnackbarTop: number;
}

interface GlobalOverlaysProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly commit: CommitOverlayProps;
	readonly discard: DiscardOverlayProps;
	readonly isHelpModalOpen: boolean;
	readonly themeModal: ThemeOverlayProps;
	readonly branchCompare: BranchCompareOverlayProps;
	readonly commitSearch: CommitSearchOverlayProps;
	readonly blameView: BlameOverlayProps;
	readonly notifications: NotificationsOverlayProps;
}

export function GlobalOverlays(props: GlobalOverlaysProps) {
	return (
		<>
			{props.commit.isOpen ? (
				<CommitModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					commitMessage={props.commit.commitMessage}
					commitError={props.commit.commitError}
					onCommitMessageChange={props.commit.onCommitMessageChange}
					onCommitSubmit={props.commit.onCommitSubmit}
				/>
			) : null}
			{props.discard.isOpen && props.discard.discardModalFile ? (
				<DiscardModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					file={props.discard.discardModalFile}
					onCancel={props.discard.onCancelDiscardModal}
					onConfirm={props.discard.onConfirmDiscardModal}
				/>
			) : null}
			{props.isHelpModalOpen ? (
				<HelpModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
				/>
			) : null}
			{props.themeModal.isOpen ? (
				<ThemeModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					themes={props.themeModal.themeNames}
					selectedThemeName={props.themeModal.selectedThemeName}
					searchQuery={props.themeModal.themeSearchQuery}
					onSearchQueryChange={props.themeModal.onSearchQueryChange}
					onSelectTheme={props.themeModal.onSelectTheme}
				/>
			) : null}
			{props.branchCompare.isOpen ? (
				<BranchCompareModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					sourceQuery={props.branchCompare.branchSourceQuery}
					destinationQuery={props.branchCompare.branchDestinationQuery}
					sourceRef={props.branchCompare.branchSourceRef}
					destinationRef={props.branchCompare.branchDestinationRef}
					activeField={props.branchCompare.branchActiveField}
					filteredRefs={props.branchCompare.branchFilteredRefs}
					selectedActiveRef={props.branchCompare.branchSelectedActiveRef}
					loading={props.branchCompare.branchModalLoading}
					error={props.branchCompare.branchModalError}
					onSourceQueryChange={props.branchCompare.onBranchSourceQueryChange}
					onDestinationQueryChange={
						props.branchCompare.onBranchDestinationQueryChange
					}
					onSelectRef={props.branchCompare.onBranchSelectRef}
					onActivateField={props.branchCompare.onBranchActivateField}
				/>
			) : null}
			{props.commitSearch.isOpen ? (
				<CommitSearchModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					query={props.commitSearch.commitSearchQuery}
					commits={props.commitSearch.commitSearchCommits}
					selectedCommitHash={props.commitSearch.commitSelectedCommitHash}
					selectedIndex={props.commitSearch.commitSelectedIndex}
					loading={props.commitSearch.commitSearchModalLoading}
					error={props.commitSearch.commitSearchModalError}
					onQueryChange={props.commitSearch.onCommitSearchQueryChange}
					onSelectCommit={props.commitSearch.onCommitSearchSelectCommit}
				/>
			) : null}
			{props.blameView.isOpen && props.blameView.blameTarget ? (
				<BlameView
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					target={props.blameView.blameTarget}
					loading={props.blameView.blameLoading}
					details={Option.fromNullable(props.blameView.blameDetails)}
					error={Option.fromNullable(props.blameView.blameError)}
					scrollRef={props.blameView.blameScrollRef}
				/>
			) : null}
			<RemoteSyncStatus
				theme={props.theme}
				state={props.notifications.remoteSync}
			/>
			<Snackbar
				theme={props.theme}
				notice={props.notifications.daemonSnackbarNotice}
				top={props.notifications.snackbarTop}
			/>
			<Snackbar
				theme={props.theme}
				notice={props.notifications.transientSnackbarNotice}
				top={props.notifications.transientSnackbarTop}
			/>
		</>
	);
}
