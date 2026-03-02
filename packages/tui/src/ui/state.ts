import { Atom } from "@effect-atom/atom-react";
import { Option } from "effect";
import type { BranchDiffSelection } from "#data/git";
import type { FileEntry } from "#tui/types";

export type UiStatus = {
	readonly showSplash: boolean;
	readonly error: Option.Option<string>;
};

export type UpdateUiStatus = (
	update: (current: UiStatus) => UiStatus,
) => void;

export const uiStatusAtom = Atom.make<UiStatus>({
	showSplash: true,
	error: Option.none(),
});

export type CommitModalState =
	| {
			readonly isOpen: false;
	  }
	| {
			readonly isOpen: true;
			readonly message: string;
			readonly error: Option.Option<string>;
	  };

export type UpdateCommitModal = (
	update: (current: CommitModalState) => CommitModalState,
) => void;

export const commitModalAtom = Atom.make<CommitModalState>({
	isOpen: false,
});

export type HelpModalState = {
	readonly isOpen: boolean;
};

export type UpdateHelpModal = (
	update: (current: HelpModalState) => HelpModalState,
) => void;

export const helpModalAtom = Atom.make<HelpModalState>({
	isOpen: false,
});

export type ThemeModalState =
	| {
			readonly isOpen: false;
	  }
	| {
			readonly isOpen: true;
			readonly initialThemeName: string;
			readonly selectedThemeName: string;
	  };

export type UpdateThemeModal = (
	update: (current: ThemeModalState) => ThemeModalState,
) => void;

export const themeModalAtom = Atom.make<ThemeModalState>({
	isOpen: false,
});

export type DiscardModalState =
	| {
			readonly isOpen: false;
	  }
	| {
			readonly isOpen: true;
			readonly file: FileEntry;
	  };

export type UpdateDiscardModal = (
	update: (current: DiscardModalState) => DiscardModalState,
) => void;

export const discardModalAtom = Atom.make<DiscardModalState>({
	isOpen: false,
});

export type RemoteSyncState =
	| {
			readonly _tag: "idle";
	  }
	| {
			readonly _tag: "running";
			readonly direction: "pull" | "push";
	  };

export type UpdateRemoteSyncState = (
	update: (current: RemoteSyncState) => RemoteSyncState,
) => void;

export const remoteSyncAtom = Atom.make<RemoteSyncState>({
	_tag: "idle",
});

export type ReviewMode =
	| {
			readonly _tag: "working-tree";
	  }
	| {
			readonly _tag: "branch-compare";
			readonly selection: BranchDiffSelection;
	  };

export type UpdateReviewMode = (
	update: (current: ReviewMode) => ReviewMode,
) => void;

export const reviewModeAtom = Atom.make<ReviewMode>({
	_tag: "working-tree",
});

export type BranchCompareField = "source" | "destination";

export type BranchCompareModalState =
	| {
			readonly isOpen: false;
	  }
	| {
			readonly isOpen: true;
			readonly loading: boolean;
			readonly availableRefs: ReadonlyArray<string>;
			readonly sourceQuery: string;
			readonly destinationQuery: string;
			readonly sourceRef: Option.Option<string>;
			readonly destinationRef: Option.Option<string>;
			readonly activeField: BranchCompareField;
			readonly selectedSourceIndex: number;
			readonly selectedDestinationIndex: number;
			readonly error: Option.Option<string>;
	  };

export type UpdateBranchCompareModal = (
	update: (current: BranchCompareModalState) => BranchCompareModalState,
) => void;

export const branchCompareModalAtom = Atom.make<BranchCompareModalState>({
	isOpen: false,
});

export interface FileViewState {
	readonly files: FileEntry[];
	readonly sidebarOpen: boolean;
	readonly diffViewMode: "split" | "unified";
	readonly collapsedDirectories: Set<string>;
	readonly selectedPath: Option.Option<string>;
	readonly loading: boolean;
}

export type UpdateFileViewState = (
	update: (current: FileViewState) => FileViewState,
) => void;

export const fileViewStateAtom = Atom.make<FileViewState>({
	files: [],
	sidebarOpen: true,
	diffViewMode: "split",
	collapsedDirectories: new Set<string>(),
	selectedPath: Option.none(),
	loading: true,
});

export interface ModalVisibility {
	readonly isCommitModalOpen: boolean;
	readonly isDiscardModalOpen: boolean;
	readonly isHelpModalOpen: boolean;
	readonly isThemeModalOpen: boolean;
	readonly isBranchCompareModalOpen: boolean;
	readonly isAnyModalOpen: boolean;
}

interface DeriveModalVisibilityOptions {
	readonly commitModal: CommitModalState;
	readonly discardModal: DiscardModalState;
	readonly helpModal: HelpModalState;
	readonly themeModal: ThemeModalState;
	readonly branchCompareModal: BranchCompareModalState;
}

export function deriveModalVisibility(
	options: DeriveModalVisibilityOptions,
): ModalVisibility {
	const isCommitModalOpen = options.commitModal.isOpen;
	const isDiscardModalOpen = options.discardModal.isOpen;
	const isHelpModalOpen = options.helpModal.isOpen;
	const isThemeModalOpen = options.themeModal.isOpen;
	const isBranchCompareModalOpen = options.branchCompareModal.isOpen;
	return {
		isCommitModalOpen,
		isDiscardModalOpen,
		isHelpModalOpen,
		isThemeModalOpen,
		isBranchCompareModalOpen,
		isAnyModalOpen:
			isCommitModalOpen ||
			isDiscardModalOpen ||
			isHelpModalOpen ||
			isThemeModalOpen ||
			isBranchCompareModalOpen,
	};
}

export function isWorkingTreeReviewMode(
	reviewMode: ReviewMode,
): reviewMode is Extract<ReviewMode, { readonly _tag: "working-tree" }> {
	return reviewMode._tag === "working-tree";
}

export function isBranchCompareReviewMode(
	reviewMode: ReviewMode,
): reviewMode is Extract<ReviewMode, { readonly _tag: "branch-compare" }> {
	return reviewMode._tag === "branch-compare";
}
