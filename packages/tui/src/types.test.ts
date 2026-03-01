import { describe, expect, test } from "bun:test";
import { FileEntry } from "#tui/types";

describe("FileEntry.equals", () => {
	test("returns true for identical entries", () => {
		const left = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			diff: "@@ -1 +1 @@",
			filetype: "typescript",
			note: "note",
		});
		const right = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			diff: "@@ -1 +1 @@",
			filetype: "typescript",
			note: "note",
		});

		expect(left.equals(right)).toBe(true);
	});

	test("returns false when one field differs", () => {
		const left = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			diff: "@@ -1 +1 @@",
		});
		const right = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			diff: "@@ -1 +2 @@",
		});

		expect(left.equals(right)).toBe(false);
	});
});
