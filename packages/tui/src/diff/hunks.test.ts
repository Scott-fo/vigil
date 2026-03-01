import { describe, expect, test } from "bun:test";
import { splitDiffIntoHunkBlocks } from "#diff/hunks";

describe("splitDiffIntoHunkBlocks", () => {
	test("returns original diff when there are no hunk headers", () => {
		const input = [
			"diff --git a/readme.md b/readme.md",
			"index abcdef..123456 100644",
			"--- a/readme.md",
			"+++ b/readme.md",
		].join("\n");

		expect(splitDiffIntoHunkBlocks(input)).toEqual([input]);
	});

	test("splits a diff into separate hunk blocks with shared header", () => {
		const input = [
			"diff --git a/src/app.ts b/src/app.ts",
			"index 111111..222222 100644",
			"--- a/src/app.ts",
			"+++ b/src/app.ts",
			"@@ -1,3 +1,3 @@",
			"-const x = 1",
			"+const x = 2",
			" unchanged",
			"@@ -20,2 +20,3 @@",
			" line",
			"+added",
		].join("\n");

		const blocks = splitDiffIntoHunkBlocks(input);
		expect(blocks).toHaveLength(2);

		expect(blocks[0]).toContain("diff --git a/src/app.ts b/src/app.ts");
		expect(blocks[0]).toContain("@@ -1,3 +1,3 @@");
		expect(blocks[0]).not.toContain("@@ -20,2 +20,3 @@");

		expect(blocks[1]).toContain("diff --git a/src/app.ts b/src/app.ts");
		expect(blocks[1]).toContain("@@ -20,2 +20,3 @@");
		expect(blocks[1]).not.toContain("@@ -1,3 +1,3 @@");
	});
});
