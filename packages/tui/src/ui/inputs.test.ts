import { describe, expect, test } from "bun:test";
import type { KeyEvent } from "@opentui/core";
import { Option } from "effect";
import { FileEntry } from "#tui/types";
import {
	decodeKeyboardIntent,
	type KeyboardIntentContext,
} from "#ui/inputs";

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
	diff: "@@ -1 +1 @@",
});

function context(
	overrides: Partial<KeyboardIntentContext> = {},
): KeyboardIntentContext {
	return {
		isCommitModalOpen: false,
		isHelpModalOpen: false,
		isThemeModalOpen: false,
		canInitializeGitRepo: false,
		stagedFileCount: 1,
		visibleFilePaths: ["src/app.tsx", "src/other.ts"],
		selectedVisibleIndex: 0,
		selectedFile,
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

	test("maps j/k to theme preview movement when theme modal is open", () => {
		const downIntent = decodeKeyboardIntent(
			keyEvent({ name: "j" }),
			context({ isThemeModalOpen: true }),
		);
		const upIntent = decodeKeyboardIntent(
			keyEvent({ name: "k" }),
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
