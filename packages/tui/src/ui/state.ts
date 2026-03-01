import { Atom } from "@effect-atom/atom-react";
import { Option } from "effect";
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
