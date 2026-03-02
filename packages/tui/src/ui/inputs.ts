import type { KeyEvent } from "@opentui/core";
import { useKeyboard } from "@opentui/react";
import { Option, pipe } from "effect";
import type { FileEntry } from "#tui/types";

interface UseAppKeyboardInputOptions {
	isCommitModalOpen: boolean;
	isDiscardModalOpen: boolean;
	isHelpModalOpen: boolean;
	isThemeModalOpen: boolean;
	canInitializeGitRepo: boolean;
	stagedFileCount: number;
	visibleFilePaths: string[];
	selectedVisibleIndex: number;
	selectedFile: FileEntry | null;
	onIntent: (intent: AppKeyboardIntent) => void;
}

export interface KeyboardIntentContext {
	isCommitModalOpen: boolean;
	isDiscardModalOpen: boolean;
	isHelpModalOpen: boolean;
	isThemeModalOpen: boolean;
	canInitializeGitRepo: boolean;
	stagedFileCount: number;
	visibleFilePaths: string[];
	selectedVisibleIndex: number;
	selectedFile: FileEntry | null;
}

export type AppKeyboardIntent =
	| { readonly _tag: "DestroyRenderer" }
	| { readonly _tag: "ToggleSidebar" }
	| { readonly _tag: "ToggleDiffViewMode" }
	| { readonly _tag: "CloseCommitModal" }
	| { readonly _tag: "OpenCommitModal" }
	| { readonly _tag: "CloseDiscardModal" }
	| { readonly _tag: "OpenDiscardModal"; readonly file: FileEntry }
	| { readonly _tag: "ConfirmDiscardModal" }
	| { readonly _tag: "CloseHelpModal" }
	| { readonly _tag: "OpenHelpModal" }
	| { readonly _tag: "InitGitRepository" }
	| { readonly _tag: "OpenThemeModal" }
	| { readonly _tag: "CloseThemeModal" }
	| { readonly _tag: "ConfirmThemeModal" }
	| { readonly _tag: "MoveThemeSelection"; readonly direction: 1 | -1 }
	| { readonly _tag: "SyncRemote"; readonly direction: "pull" | "push" }
	| { readonly _tag: "ScrollDiffHalfPage"; readonly direction: "up" | "down" }
	| { readonly _tag: "OpenSelectedFile"; readonly filePath: string }
	| { readonly _tag: "ToggleSelectedFileStage"; readonly file: FileEntry }
	| { readonly _tag: "SelectVisiblePath"; readonly path: string };

function isUnmodifiedKey(key: KeyEvent, name: string): boolean {
	return !key.ctrl && !key.meta && key.name === name;
}

function isQuestionMarkKey(key: KeyEvent): boolean {
	if (key.ctrl || key.meta) {
		return false;
	}
	return key.name === "?" || (key.name === "/" && key.shift);
}

export function decodeKeyboardIntent(
	key: KeyEvent,
	options: KeyboardIntentContext,
): Option.Option<AppKeyboardIntent> {
	if (key.ctrl && key.name === "c") {
		return Option.some({ _tag: "DestroyRenderer" });
	}

	if (key.ctrl && key.name === "b") {
		return Option.some({ _tag: "ToggleSidebar" });
	}

	if (options.isCommitModalOpen) {
		return key.name === "escape"
			? Option.some({ _tag: "CloseCommitModal" })
			: Option.none();
	}

	if (options.isDiscardModalOpen) {
		if (key.name === "escape") {
			return Option.some({ _tag: "CloseDiscardModal" });
		}
		if (key.name === "enter" || key.name === "return") {
			return Option.some({ _tag: "ConfirmDiscardModal" });
		}
		return Option.none();
	}

	if (options.isHelpModalOpen) {
		return key.name === "escape"
			? Option.some({ _tag: "CloseHelpModal" })
			: Option.none();
	}

	if (options.isThemeModalOpen) {
		if (key.name === "escape") {
			return Option.some({ _tag: "CloseThemeModal" });
		}
		if (key.name === "enter" || key.name === "return") {
			return Option.some({ _tag: "ConfirmThemeModal" });
		}
		if (key.name === "down") {
			return Option.some({ _tag: "MoveThemeSelection", direction: 1 });
		}
		if (key.name === "up") {
			return Option.some({ _tag: "MoveThemeSelection", direction: -1 });
		}
		return Option.none();
	}

	if (key.name === "escape" || key.name === "q") {
		return Option.some({ _tag: "DestroyRenderer" });
	}

	if (isQuestionMarkKey(key)) {
		return Option.some({ _tag: "OpenHelpModal" });
	}

	if (isUnmodifiedKey(key, "i") && options.canInitializeGitRepo) {
		return Option.some({ _tag: "InitGitRepository" });
	}

	if (isUnmodifiedKey(key, "c") && options.stagedFileCount > 0) {
		return Option.some({ _tag: "OpenCommitModal" });
	}

	if (isUnmodifiedKey(key, "d")) {
		return pipe(
			Option.fromNullable(options.selectedFile),
			Option.map((selectedFile) => ({
				_tag: "OpenDiscardModal" as const,
				file: selectedFile,
			})),
		);
	}

	if (isUnmodifiedKey(key, "t")) {
		return Option.some({ _tag: "OpenThemeModal" });
	}

	if (isUnmodifiedKey(key, "p")) {
		return Option.some({
			_tag: "SyncRemote",
			direction: key.shift ? "push" : "pull",
		});
	}

	if (key.name === "tab" && !key.ctrl && !key.meta) {
		return Option.some({ _tag: "ToggleDiffViewMode" });
	}

	if (key.ctrl && (key.name === "u" || key.name === "d")) {
		return Option.some({
			_tag: "ScrollDiffHalfPage",
			direction: key.name === "u" ? "up" : "down",
		});
	}

	const hasVisibleSelection =
		options.visibleFilePaths.length > 0 && options.selectedVisibleIndex !== -1;
	if (!hasVisibleSelection) {
		return Option.none();
	}

	if (
		key.name === "enter" ||
		key.name === "return" ||
		isUnmodifiedKey(key, "e") ||
		isUnmodifiedKey(key, "o")
	) {
		return pipe(
			Option.fromNullable(options.selectedFile),
			Option.map((selectedFile) => ({
				_tag: "OpenSelectedFile" as const,
				filePath: selectedFile.path,
			})),
		);
	}

	if (!key.ctrl && !key.meta && (key.name === "space" || key.name === " ")) {
		return pipe(
			Option.fromNullable(options.selectedFile),
			Option.map((selectedFile) => ({
				_tag: "ToggleSelectedFileStage" as const,
				file: selectedFile,
			})),
		);
	}

	if (key.name === "down" || key.name === "j") {
		const nextIndex = Math.min(
			options.selectedVisibleIndex + 1,
			options.visibleFilePaths.length - 1,
		);
		return pipe(
			Option.fromNullable(options.visibleFilePaths[nextIndex]),
			Option.map((path) => ({ _tag: "SelectVisiblePath" as const, path })),
		);
	}

	if (key.name === "up" || key.name === "k") {
		const nextIndex = Math.max(options.selectedVisibleIndex - 1, 0);
		return pipe(
			Option.fromNullable(options.visibleFilePaths[nextIndex]),
			Option.map((path) => ({ _tag: "SelectVisiblePath" as const, path })),
		);
	}

	return Option.none();
}

export function useAppKeyboardInput(options: UseAppKeyboardInputOptions) {
	useKeyboard((key) => {
		pipe(
			decodeKeyboardIntent(key, options),
			Option.match({
				onNone: () => {},
				onSome: (intent) => {
					options.onIntent(intent);
				},
			}),
		);
	});
}
