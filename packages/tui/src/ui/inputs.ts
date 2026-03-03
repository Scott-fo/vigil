import type { KeyEvent } from "@opentui/core";
import { useKeyboard } from "@opentui/react";
import { Option, pipe } from "effect";
import { useRef } from "react";
import type { FileEntry } from "#tui/types";

export type FocusedPane = "sidebar" | "diff";

interface UseAppKeyboardInputOptions {
	isCommitModalOpen: boolean;
	isDiscardModalOpen: boolean;
	isHelpModalOpen: boolean;
	isThemeModalOpen: boolean;
	isBranchCompareModalOpen: boolean;
	isBranchCompareMode: boolean;
	activePane: FocusedPane;
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
	isBranchCompareModalOpen: boolean;
	isBranchCompareMode: boolean;
	activePane: FocusedPane;
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
	| { readonly _tag: "OpenBranchCompareModal" }
	| { readonly _tag: "CloseThemeModal" }
	| { readonly _tag: "CloseBranchCompareModal" }
	| { readonly _tag: "ConfirmThemeModal" }
	| { readonly _tag: "ConfirmBranchCompareModal" }
	| { readonly _tag: "MoveThemeSelection"; readonly direction: 1 | -1 }
	| { readonly _tag: "MoveBranchSelection"; readonly direction: 1 | -1 }
	| { readonly _tag: "SwitchBranchModalField" }
	| { readonly _tag: "SyncRemote"; readonly direction: "pull" | "push" }
	| { readonly _tag: "ResetReviewMode" }
	| { readonly _tag: "ScrollDiffHalfPage"; readonly direction: "up" | "down" }
	| { readonly _tag: "FocusSidebarPane" }
	| { readonly _tag: "FocusDiffPane" }
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

function isPaneFocusChordStart(key: KeyEvent): boolean {
	return key.ctrl && !key.meta && key.name === "w";
}

export function decodePaneFocusIntent(
	key: KeyEvent,
): Option.Option<AppKeyboardIntent> {
	if (key.ctrl || key.meta) {
		return Option.none();
	}

	if (key.name === "h" || key.name === "left") {
		return Option.some({ _tag: "FocusSidebarPane" });
	}

	if (key.name === "l" || key.name === "right") {
		return Option.some({ _tag: "FocusDiffPane" });
	}

	return Option.none();
}

type DecoderLayerResult =
	| { readonly _tag: "pass" }
	| {
			readonly _tag: "handled";
			readonly intent: Option.Option<AppKeyboardIntent>;
	  };

function passLayer(): DecoderLayerResult {
	return { _tag: "pass" };
}

function handledLayer(
	intent: Option.Option<AppKeyboardIntent>,
): DecoderLayerResult {
	return { _tag: "handled", intent };
}

function decodePriorityGlobalIntent(
	key: KeyEvent,
): Option.Option<AppKeyboardIntent> {
	if (key.ctrl && key.name === "c") {
		return Option.some({ _tag: "DestroyRenderer" });
	}

	if (key.ctrl && key.name === "b") {
		return Option.some({ _tag: "ToggleSidebar" });
	}

	return Option.none();
}

function decodeModalIntentLayer(
	key: KeyEvent,
	options: KeyboardIntentContext,
): DecoderLayerResult {
	if (options.isCommitModalOpen) {
		return handledLayer(
			key.name === "escape"
				? Option.some({ _tag: "CloseCommitModal" })
				: Option.none(),
		);
	}

	if (options.isDiscardModalOpen) {
		if (key.name === "escape") {
			return handledLayer(Option.some({ _tag: "CloseDiscardModal" }));
		}
		if (key.name === "enter" || key.name === "return") {
			return handledLayer(Option.some({ _tag: "ConfirmDiscardModal" }));
		}
		return handledLayer(Option.none());
	}

	if (options.isHelpModalOpen) {
		return handledLayer(
			key.name === "escape"
				? Option.some({ _tag: "CloseHelpModal" })
				: Option.none(),
		);
	}

	if (options.isThemeModalOpen) {
		if (key.name === "escape") {
			return handledLayer(Option.some({ _tag: "CloseThemeModal" }));
		}
		if (key.name === "enter" || key.name === "return") {
			return handledLayer(Option.some({ _tag: "ConfirmThemeModal" }));
		}
		if (key.name === "down") {
			return handledLayer(
				Option.some({ _tag: "MoveThemeSelection", direction: 1 }),
			);
		}
		if (key.name === "up") {
			return handledLayer(
				Option.some({ _tag: "MoveThemeSelection", direction: -1 }),
			);
		}
		return handledLayer(Option.none());
	}

	if (options.isBranchCompareModalOpen) {
		if (key.name === "escape") {
			return handledLayer(Option.some({ _tag: "CloseBranchCompareModal" }));
		}
		if (key.name === "enter" || key.name === "return") {
			return handledLayer(Option.some({ _tag: "ConfirmBranchCompareModal" }));
		}
		if (key.name === "tab" && !key.ctrl && !key.meta) {
			return handledLayer(Option.some({ _tag: "SwitchBranchModalField" }));
		}
		if (key.name === "down") {
			return handledLayer(
				Option.some({ _tag: "MoveBranchSelection", direction: 1 }),
			);
		}
		if (key.name === "up") {
			return handledLayer(
				Option.some({ _tag: "MoveBranchSelection", direction: -1 }),
			);
		}
		return handledLayer(Option.none());
	}

	return passLayer();
}

function decodeGlobalIntentLayer(
	key: KeyEvent,
	options: KeyboardIntentContext,
): Option.Option<AppKeyboardIntent> {
	if (key.name === "escape" || key.name === "q") {
		return Option.some({ _tag: "DestroyRenderer" });
	}

	if (isQuestionMarkKey(key)) {
		return Option.some({ _tag: "OpenHelpModal" });
	}

	if (key.ctrl && key.name === "l") {
		return Option.some({ _tag: "ResetReviewMode" });
	}

	if (isUnmodifiedKey(key, "i") && options.canInitializeGitRepo) {
		return Option.some({ _tag: "InitGitRepository" });
	}

	if (isUnmodifiedKey(key, "c") && options.stagedFileCount > 0) {
		return options.isBranchCompareMode
			? Option.none()
			: Option.some({ _tag: "OpenCommitModal" });
	}

	if (isUnmodifiedKey(key, "d")) {
		if (options.isBranchCompareMode) {
			return Option.none();
		}
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

	if (isUnmodifiedKey(key, "b")) {
		return Option.some({ _tag: "OpenBranchCompareModal" });
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

	return Option.none();
}

function decodeSelectionIntentLayer(
	key: KeyEvent,
	options: KeyboardIntentContext,
): Option.Option<AppKeyboardIntent> {
	if (options.activePane !== "sidebar") {
		return Option.none();
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
		if (options.isBranchCompareMode) {
			return Option.none();
		}
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

export function decodeKeyboardIntent(
	key: KeyEvent,
	options: KeyboardIntentContext,
): Option.Option<AppKeyboardIntent> {
	const priorityGlobalIntent = decodePriorityGlobalIntent(key);
	if (Option.isSome(priorityGlobalIntent)) {
		return priorityGlobalIntent;
	}

	const modalIntent = decodeModalIntentLayer(key, options);
	if (modalIntent._tag === "handled") {
		return modalIntent.intent;
	}

	const globalIntent = decodeGlobalIntentLayer(key, options);
	if (Option.isSome(globalIntent)) {
		return globalIntent;
	}

	return decodeSelectionIntentLayer(key, options);
}

export function useAppKeyboardInput(options: UseAppKeyboardInputOptions) {
	const pendingPaneFocusRef = useRef(false);

	useKeyboard((key) => {
		const hasOpenModal =
			options.isCommitModalOpen ||
			options.isDiscardModalOpen ||
			options.isHelpModalOpen ||
			options.isThemeModalOpen ||
			options.isBranchCompareModalOpen;

		if (pendingPaneFocusRef.current) {
			pendingPaneFocusRef.current = false;

			const priorityGlobalIntent = decodePriorityGlobalIntent(key);
			if (Option.isSome(priorityGlobalIntent)) {
				options.onIntent(priorityGlobalIntent.value);
				return;
			}

			if (hasOpenModal) {
				return;
			}

			pipe(
				decodePaneFocusIntent(key),
				Option.match({
					onNone: () => {},
					onSome: (intent) => {
						options.onIntent(intent);
					},
				}),
			);
			return;
		}

		if (!hasOpenModal && isPaneFocusChordStart(key)) {
			pendingPaneFocusRef.current = true;
			return;
		}

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
