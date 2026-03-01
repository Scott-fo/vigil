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
