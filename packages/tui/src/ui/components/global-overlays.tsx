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

interface GlobalOverlaysProps {
	readonly theme: ResolvedTheme;
	readonly modalBackdropColor: RGBA;
	readonly isCommitModalOpen: boolean;
	readonly commitMessage: string;
	readonly commitError: Option.Option<string>;
	readonly onCommitMessageChange: (value: string) => void;
	readonly onCommitSubmit: (payload: unknown) => void;
	readonly isDiscardModalOpen: boolean;
	readonly discardModalFile: FileEntry | null;
	readonly onCancelDiscardModal: () => void;
	readonly onConfirmDiscardModal: () => void;
	readonly isHelpModalOpen: boolean;
	readonly isThemeModalOpen: boolean;
	readonly themeNames: ReadonlyArray<string>;
	readonly selectedThemeName: string;
	readonly themeSearchQuery: string;
	readonly onSearchQueryChange: (value: string) => void;
	readonly onSelectTheme: (themeName: string) => void;
	readonly isBranchCompareModalOpen: boolean;
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
	readonly isCommitSearchModalOpen: boolean;
	readonly commitSearchQuery: string;
	readonly commitSearchCommits: ReadonlyArray<CommitDiffSelection>;
	readonly commitSelectedCommitHash: Option.Option<string>;
	readonly commitSelectedIndex: number;
	readonly commitSearchModalLoading: boolean;
	readonly commitSearchModalError: Option.Option<string>;
	readonly onCommitSearchQueryChange: (query: string) => void;
	readonly onCommitSearchSelectCommit: (commitHash: string) => void;
	readonly isBlameViewOpen: boolean;
	readonly blameTarget: BlameTarget | null;
	readonly blameLoading: boolean;
	readonly blameDetails: BlameCommitDetails | null;
	readonly blameError: string | null;
	readonly blameScrollRef: RefObject<ScrollBoxRenderable | null>;
	readonly remoteSync: RemoteSyncState;
	readonly daemonSnackbarNotice: Option.Option<SnackbarNotice>;
	readonly transientSnackbarNotice: Option.Option<SnackbarNotice>;
	readonly snackbarTop: number;
	readonly transientSnackbarTop: number;
}

export function GlobalOverlays(props: GlobalOverlaysProps) {
	return (
		<>
			{props.isCommitModalOpen ? (
				<CommitModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					commitMessage={props.commitMessage}
					commitError={props.commitError}
					onCommitMessageChange={props.onCommitMessageChange}
					onCommitSubmit={props.onCommitSubmit}
				/>
			) : null}
			{props.isDiscardModalOpen && props.discardModalFile ? (
				<DiscardModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					file={props.discardModalFile}
					onCancel={props.onCancelDiscardModal}
					onConfirm={props.onConfirmDiscardModal}
				/>
			) : null}
			{props.isHelpModalOpen ? (
				<HelpModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
				/>
			) : null}
			{props.isThemeModalOpen ? (
				<ThemeModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					themes={props.themeNames}
					selectedThemeName={props.selectedThemeName}
					searchQuery={props.themeSearchQuery}
					onSearchQueryChange={props.onSearchQueryChange}
					onSelectTheme={props.onSelectTheme}
				/>
			) : null}
			{props.isBranchCompareModalOpen ? (
				<BranchCompareModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					sourceQuery={props.branchSourceQuery}
					destinationQuery={props.branchDestinationQuery}
					sourceRef={props.branchSourceRef}
					destinationRef={props.branchDestinationRef}
					activeField={props.branchActiveField}
					filteredRefs={props.branchFilteredRefs}
					selectedActiveRef={props.branchSelectedActiveRef}
					loading={props.branchModalLoading}
					error={props.branchModalError}
					onSourceQueryChange={props.onBranchSourceQueryChange}
					onDestinationQueryChange={props.onBranchDestinationQueryChange}
					onSelectRef={props.onBranchSelectRef}
					onActivateField={props.onBranchActivateField}
				/>
			) : null}
			{props.isCommitSearchModalOpen ? (
				<CommitSearchModal
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					query={props.commitSearchQuery}
					commits={props.commitSearchCommits}
					selectedCommitHash={props.commitSelectedCommitHash}
					selectedIndex={props.commitSelectedIndex}
					loading={props.commitSearchModalLoading}
					error={props.commitSearchModalError}
					onQueryChange={props.onCommitSearchQueryChange}
					onSelectCommit={props.onCommitSearchSelectCommit}
				/>
			) : null}
			{props.isBlameViewOpen && props.blameTarget ? (
				<BlameView
					theme={props.theme}
					modalBackdropColor={props.modalBackdropColor}
					target={props.blameTarget}
					loading={props.blameLoading}
					details={Option.fromNullable(props.blameDetails)}
					error={Option.fromNullable(props.blameError)}
					scrollRef={props.blameScrollRef}
				/>
			) : null}
			<RemoteSyncStatus theme={props.theme} state={props.remoteSync} />
			<Snackbar
				theme={props.theme}
				notice={props.daemonSnackbarNotice}
				top={props.snackbarTop}
			/>
			<Snackbar
				theme={props.theme}
				notice={props.transientSnackbarNotice}
				top={props.transientSnackbarTop}
			/>
		</>
	);
}
