import { describe, expect, test } from "bun:test";
import { FileEntry } from "#tui/types.ts";

describe("FileEntry.equals", () => {
	test("returns true for identical entries", () => {
		const left = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			filetype: "typescript",
		});
		const right = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			filetype: "typescript",
		});

		expect(left.equals(right)).toBe(true);
	});

	test("returns false when one field differs", () => {
		const left = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			filetype: "typescript",
		});
		const right = FileEntry.make({
			status: "M ",
			path: "src/app.tsx",
			label: "src/app.tsx",
			filetype: "tsx",
		});

		expect(left.equals(right)).toBe(false);
	});
});
