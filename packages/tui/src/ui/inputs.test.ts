import { describe, expect, test } from "bun:test";
import type { KeyEvent } from "@opentui/core";
import { Option } from "effect";
import { FileEntry } from "#tui/types.ts";
import {
	decodeKeyboardIntent,
	decodePaneFocusIntent,
	type KeyboardIntentContext,
} from "#ui/inputs.ts";

function keyEvent(input: Partial<KeyEvent>): KeyEvent {
	return {
		name: "",
		sequence: "",
		ctrl: false,
		meta: false,
		shift: false,
		...input,
	} as unknown as KeyEvent;
}

const selectedFile = FileEntry.make({
	status: "M ",
	path: "src/app.tsx",
	label: "src/app.tsx",
});

function context(
	overrides: Partial<KeyboardIntentContext> = {},
): KeyboardIntentContext {
	return {
		isCommitModalOpen: false,
		isDiscardModalOpen: false,
		isHelpModalOpen: false,
		isThemeModalOpen: false,
		isBranchCompareModalOpen: false,
		isBranchCompareMode: false,
		activePane: "sidebar",
		canInitializeGitRepo: false,
		stagedFileCount: 1,
		visibleFilePaths: ["src/app.tsx", "src/other.ts"],
		selectedVisibleIndex: 0,
		selectedFile,
		selectedDiffFilePath: "src/app.tsx",
		selectedDiffLineNumber: 12,
		...overrides,
	};
}

describe("decodeKeyboardIntent", () => {
	test("maps ctrl+c to destroy renderer", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "c", ctrl: true }),
			context(),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("DestroyRenderer");
		}
	});

	test("opens commit modal only when staged files exist", () => {
		const allowed = decodeKeyboardIntent(keyEvent({ name: "c" }), context());
		const blocked = decodeKeyboardIntent(
			keyEvent({ name: "c" }),
			context({ stagedFileCount: 0 }),
		);

		expect(Option.isSome(allowed)).toBe(true);
		if (Option.isSome(allowed)) {
			expect(allowed.value._tag).toBe("OpenCommitModal");
		}
		expect(Option.isNone(blocked)).toBe(true);
	});

	test("maps ctrl+b to sidebar toggle", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "b", ctrl: true }),
			context(),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("ToggleSidebar");
		}
	});

	test("maps tab to diff view toggle", () => {
		const intent = decodeKeyboardIntent(keyEvent({ name: "tab" }), context());
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("ToggleDiffViewMode");
		}
	});

	test("maps d to open discard modal for selected file", () => {
		const intent = decodeKeyboardIntent(keyEvent({ name: "d" }), context());
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value).toEqual({
				_tag: "OpenDiscardModal",
				file: selectedFile,
			});
		}
	});

	test("maps escape to close discard modal when open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "escape" }),
			context({ isDiscardModalOpen: true }),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("CloseDiscardModal");
		}
	});

	test("maps return to confirm discard modal when open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "return" }),
			context({ isDiscardModalOpen: true }),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("ConfirmDiscardModal");
		}
	});

	test("maps ? to open help modal", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "?", shift: true }),
			context(),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("OpenHelpModal");
		}
	});

	test("maps shift+/ to open help modal", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "/", shift: true }),
			context(),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("OpenHelpModal");
		}
	});

	test("maps t to open theme modal", () => {
		const intent = decodeKeyboardIntent(keyEvent({ name: "t" }), context());
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("OpenThemeModal");
		}
	});

	test("maps b to open branch compare modal", () => {
		const intent = decodeKeyboardIntent(keyEvent({ name: "b" }), context());
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("OpenBranchCompareModal");
		}
	});

	test("maps ctrl+l to reset review mode", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "l", ctrl: true }),
			context(),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("ResetReviewMode");
		}
	});

	test("maps escape to close theme modal when open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "escape" }),
			context({ isThemeModalOpen: true }),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("CloseThemeModal");
		}
	});

	test("maps return to confirm theme modal when open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "return" }),
			context({ isThemeModalOpen: true }),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("ConfirmThemeModal");
		}
	});

	test("maps up/down to theme preview movement when theme modal is open", () => {
		const downIntent = decodeKeyboardIntent(
			keyEvent({ name: "down" }),
			context({ isThemeModalOpen: true }),
		);
		const upIntent = decodeKeyboardIntent(
			keyEvent({ name: "up" }),
			context({ isThemeModalOpen: true }),
		);
		expect(Option.isSome(downIntent)).toBe(true);
		expect(Option.isSome(upIntent)).toBe(true);
		if (Option.isSome(downIntent)) {
			expect(downIntent.value).toEqual({
				_tag: "MoveThemeSelection",
				direction: 1,
			});
		}
		if (Option.isSome(upIntent)) {
			expect(upIntent.value).toEqual({
				_tag: "MoveThemeSelection",
				direction: -1,
			});
		}
	});

	test("does not map j/k when theme modal is open", () => {
		const downIntent = decodeKeyboardIntent(
			keyEvent({ name: "j" }),
			context({ isThemeModalOpen: true }),
		);
		const upIntent = decodeKeyboardIntent(
			keyEvent({ name: "k" }),
			context({ isThemeModalOpen: true }),
		);
		expect(Option.isNone(downIntent)).toBe(true);
		expect(Option.isNone(upIntent)).toBe(true);
	});

	test("handles branch compare modal navigation keys", () => {
		const downIntent = decodeKeyboardIntent(
			keyEvent({ name: "down" }),
			context({ isBranchCompareModalOpen: true }),
		);
		const tabIntent = decodeKeyboardIntent(
			keyEvent({ name: "tab" }),
			context({ isBranchCompareModalOpen: true }),
		);
		const enterIntent = decodeKeyboardIntent(
			keyEvent({ name: "return" }),
			context({ isBranchCompareModalOpen: true }),
		);
		expect(Option.isSome(downIntent)).toBe(true);
		expect(Option.isSome(tabIntent)).toBe(true);
		expect(Option.isSome(enterIntent)).toBe(true);
		if (Option.isSome(downIntent)) {
			expect(downIntent.value).toEqual({
				_tag: "MoveBranchSelection",
				direction: 1,
			});
		}
		if (Option.isSome(tabIntent)) {
			expect(tabIntent.value._tag).toBe("SwitchBranchModalField");
		}
		if (Option.isSome(enterIntent)) {
			expect(enterIntent.value._tag).toBe("ConfirmBranchCompareModal");
		}
	});

	test("maps escape to close branch compare modal when open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "escape" }),
			context({ isBranchCompareModalOpen: true }),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("CloseBranchCompareModal");
		}
	});

	test("does not map ctrl+l when branch compare modal is open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "l", ctrl: true }),
			context({ isBranchCompareModalOpen: true }),
		);
		expect(Option.isNone(intent)).toBe(true);
	});

	test("blocks staging/discard/commit shortcuts in branch compare mode", () => {
		const commitIntent = decodeKeyboardIntent(
			keyEvent({ name: "c" }),
			context({ isBranchCompareMode: true }),
		);
		const discardIntent = decodeKeyboardIntent(
			keyEvent({ name: "d" }),
			context({ isBranchCompareMode: true }),
		);
		const stageIntent = decodeKeyboardIntent(
			keyEvent({ name: "space" }),
			context({ isBranchCompareMode: true }),
		);
		expect(Option.isNone(commitIntent)).toBe(true);
		expect(Option.isNone(discardIntent)).toBe(true);
		expect(Option.isNone(stageIntent)).toBe(true);
	});

	test("maps i to init git repository when allowed", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "i" }),
			context({
				canInitializeGitRepo: true,
				visibleFilePaths: [],
				selectedVisibleIndex: -1,
				selectedFile: null,
			}),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("InitGitRepository");
		}
	});

	test("does not map i when init is not allowed", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "i" }),
			context({
				canInitializeGitRepo: false,
				visibleFilePaths: [],
				selectedVisibleIndex: -1,
				selectedFile: null,
			}),
		);
		expect(Option.isNone(intent)).toBe(true);
	});

	test("maps escape to close help modal when help is open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "escape" }),
			context({ isHelpModalOpen: true }),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value._tag).toBe("CloseHelpModal");
		}
	});

	test("ignores non-escape keys when help modal is open", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "j" }),
			context({ isHelpModalOpen: true }),
		);
		expect(Option.isNone(intent)).toBe(true);
	});

	test("maps ctrl+d to half-page down scroll intent", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "d", ctrl: true }),
			context(),
		);
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value).toEqual({
				_tag: "ScrollDiffHalfPage",
				direction: "down",
			});
		}
	});

	test("maps e to open selected file in editor", () => {
		const intent = decodeKeyboardIntent(keyEvent({ name: "e" }), context());
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value).toEqual({
				_tag: "OpenSelectedFile",
				filePath: "src/app.tsx",
			});
		}
	});

	test("maps o to open selected file in editor", () => {
		const intent = decodeKeyboardIntent(keyEvent({ name: "o" }), context());
		expect(Option.isSome(intent)).toBe(true);
		if (Option.isSome(intent)) {
			expect(intent.value).toEqual({
				_tag: "OpenSelectedFile",
				filePath: "src/app.tsx",
			});
		}
	});

	test("maps enter to open selected diff line when diff pane is focused", () => {
		const openIntent = decodeKeyboardIntent(
			keyEvent({ name: "return" }),
			context({ activePane: "diff" }),
		);
		const stageIntent = decodeKeyboardIntent(
			keyEvent({ name: "space" }),
			context({ activePane: "diff" }),
		);
		const moveIntent = decodeKeyboardIntent(
			keyEvent({ name: "j" }),
			context({ activePane: "diff" }),
		);
		const upIntent = decodeKeyboardIntent(
			keyEvent({ name: "k" }),
			context({ activePane: "diff" }),
		);

		expect(Option.isSome(openIntent)).toBe(true);
		expect(Option.isNone(stageIntent)).toBe(true);
		expect(Option.isSome(moveIntent)).toBe(true);
		expect(Option.isSome(upIntent)).toBe(true);
		if (Option.isSome(openIntent)) {
			expect(openIntent.value).toEqual({
				_tag: "OpenSelectedDiffLine",
				filePath: "src/app.tsx",
				lineNumber: 12,
			});
		}
		if (Option.isSome(moveIntent)) {
			expect(moveIntent.value).toEqual({
				_tag: "MoveDiffLineSelection",
				direction: 1,
			});
		}
		if (Option.isSome(upIntent)) {
			expect(upIntent.value).toEqual({
				_tag: "MoveDiffLineSelection",
				direction: -1,
			});
		}
	});

	test("does not map open diff line when line metadata is unavailable", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "return" }),
			context({
				activePane: "diff",
				selectedDiffLineNumber: null,
			}),
		);

		expect(Option.isNone(intent)).toBe(true);
	});

	test("maps arrow keys to diff line navigation when diff pane is focused", () => {
		const downIntent = decodeKeyboardIntent(
			keyEvent({ name: "down" }),
			context({ activePane: "diff" }),
		);
		const upIntent = decodeKeyboardIntent(
			keyEvent({ name: "up" }),
			context({ activePane: "diff" }),
		);

		expect(Option.isSome(downIntent)).toBe(true);
		expect(Option.isSome(upIntent)).toBe(true);
		if (Option.isSome(downIntent)) {
			expect(downIntent.value).toEqual({
				_tag: "MoveDiffLineSelection",
				direction: 1,
			});
		}
		if (Option.isSome(upIntent)) {
			expect(upIntent.value).toEqual({
				_tag: "MoveDiffLineSelection",
				direction: -1,
			});
		}
	});

	test("maps pane focus keys for ctrl+w chord follow-up", () => {
		const sidebarByLetter = decodePaneFocusIntent(keyEvent({ name: "h" }));
		const sidebarByArrow = decodePaneFocusIntent(keyEvent({ name: "left" }));
		const diffByLetter = decodePaneFocusIntent(keyEvent({ name: "l" }));
		const diffByArrow = decodePaneFocusIntent(keyEvent({ name: "right" }));

		expect(Option.isSome(sidebarByLetter)).toBe(true);
		expect(Option.isSome(sidebarByArrow)).toBe(true);
		expect(Option.isSome(diffByLetter)).toBe(true);
		expect(Option.isSome(diffByArrow)).toBe(true);

		if (Option.isSome(sidebarByLetter)) {
			expect(sidebarByLetter.value._tag).toBe("FocusSidebarPane");
		}
		if (Option.isSome(sidebarByArrow)) {
			expect(sidebarByArrow.value._tag).toBe("FocusSidebarPane");
		}
		if (Option.isSome(diffByLetter)) {
			expect(diffByLetter.value._tag).toBe("FocusDiffPane");
		}
		if (Option.isSome(diffByArrow)) {
			expect(diffByArrow.value._tag).toBe("FocusDiffPane");
		}
	});

	test("does not map pane focus keys with modifiers", () => {
		const intent = decodePaneFocusIntent(keyEvent({ name: "h", ctrl: true }));
		expect(Option.isNone(intent)).toBe(true);
	});

	test("returns none when no visible selection exists", () => {
		const intent = decodeKeyboardIntent(
			keyEvent({ name: "return" }),
			context({
				visibleFilePaths: [],
				selectedVisibleIndex: -1,
				selectedFile: null,
			}),
		);
		expect(Option.isNone(intent)).toBe(true);
	});
});
