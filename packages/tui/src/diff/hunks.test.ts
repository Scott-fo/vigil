import { describe, expect, test } from "bun:test";
import {
	buildDiffHunkModel,
	buildExpandedDiffHunkBlock,
	buildExpandedDiffHunkBlockRange,
	expandDiffGap,
	splitDiffIntoHunkBlocks,
} from "#diff/hunks.ts";

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

describe("buildDiffHunkModel", () => {
	test("returns hunk metadata and omitted gap ranges between hunks", () => {
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

		const model = buildDiffHunkModel(input);

		expect(model.hunks).toHaveLength(2);
		expect(model.hunks[0]).toMatchObject({
			hunkIndex: 0,
			header: "@@ -1,3 +1,3 @@",
			oldStart: 1,
			oldCount: 3,
			newStart: 1,
			newCount: 3,
		});
		expect(model.hunks[1]).toMatchObject({
			hunkIndex: 1,
			header: "@@ -20,2 +20,3 @@",
			oldStart: 20,
			oldCount: 2,
			newStart: 20,
			newCount: 3,
		});
		expect(model.gaps).toEqual([
			{
				previousHunkIndex: 0,
				nextHunkIndex: 1,
				oldStart: 4,
				oldCount: 16,
				newStart: 4,
				newCount: 16,
			},
		]);
	});

	test("omits gap entries when hunks are adjacent", () => {
		const input = [
			"diff --git a/src/app.ts b/src/app.ts",
			"index 111111..222222 100644",
			"--- a/src/app.ts",
			"+++ b/src/app.ts",
			"@@ -1,1 +1,1 @@",
			"-before",
			"+after",
			"@@ -2,1 +2,1 @@",
			"-next-before",
			"+next-after",
		].join("\n");

		expect(buildDiffHunkModel(input).gaps).toEqual([]);
	});

	test("renders expanded context from the collapsed gap into adjacent hunks", () => {
		const input = [
			"diff --git a/src/app.ts b/src/app.ts",
			"index 111111..222222 100644",
			"--- a/src/app.ts",
			"+++ b/src/app.ts",
			"@@ -2,2 +2,2 @@ function run() {",
			"-before",
			"+after",
			" unchanged",
			"@@ -8,2 +8,2 @@ function end() {",
			" stable",
			"-old",
			"+new",
		].join("\n");
		const fileLines = [
			"intro",
			"function run() {",
			"after",
			"unchanged",
			"middle-1",
			"middle-2",
			"middle-3",
			"stable",
			"new",
			"}",
		];
		const model = buildDiffHunkModel(input);
		const firstGap = model.gaps[0];
		const firstHunk = model.hunks[0];
		const secondHunk = model.hunks[1];
		if (!firstGap || !firstHunk || !secondHunk) {
			throw new Error("Expected test diff model to contain two hunks and one gap.");
		}

		const expandedFromFirst = expandDiffGap(firstGap, "down", undefined, 2);
		const expandedFromSecond = expandDiffGap(firstGap, "up", expandedFromFirst, 1);

		expect(
			buildExpandedDiffHunkBlock(
				model,
				firstHunk,
				fileLines,
				undefined,
				expandedFromSecond,
			),
		).toContain(" middle-1");
		expect(
			buildExpandedDiffHunkBlock(
				model,
				secondHunk,
				fileLines,
				expandedFromSecond,
				undefined,
			),
		).toContain(" middle-3\n stable");
	});

	test("caps gap expansion to the remaining collapsed lines", () => {
		const gap = {
			previousHunkIndex: 0,
			nextHunkIndex: 1,
			oldStart: 10,
			oldCount: 3,
			newStart: 10,
			newCount: 3,
		} as const;

		const initial = expandDiffGap(gap, "down", undefined, 2);
		const next = expandDiffGap(gap, "up", initial, 20);

		expect(initial).toEqual({
			fromPrevious: 2,
			fromNext: 0,
		});
		expect(next).toEqual({
			fromPrevious: 2,
			fromNext: 1,
		});
	});

	test("merges adjacent hunks into one rendered block when the gap is fully expanded", () => {
		const input = [
			"diff --git a/src/app.ts b/src/app.ts",
			"index 111111..222222 100644",
			"--- a/src/app.ts",
			"+++ b/src/app.ts",
			"@@ -2,2 +2,2 @@ function run() {",
			"-before",
			"+after",
			" unchanged",
			"@@ -8,2 +8,2 @@ function end() {",
			" stable",
			"-old",
			"+new",
		].join("\n");
		const fileLines = [
			"intro",
			"function run() {",
			"after",
			"unchanged",
			"middle-1",
			"middle-2",
			"middle-3",
			"stable",
			"new",
			"}",
		];
		const model = buildDiffHunkModel(input);
		const firstGap = model.gaps[0];
		const firstHunk = model.hunks[0];
		const secondHunk = model.hunks[1];
		if (!firstGap || !firstHunk || !secondHunk) {
			throw new Error("Expected test diff model to contain two hunks and one gap.");
		}

		const fullyExpanded = expandDiffGap(firstGap, "down", undefined, firstGap.newCount);
		const merged = buildExpandedDiffHunkBlockRange(
			model,
			[firstHunk, secondHunk],
			fileLines,
			undefined,
			fullyExpanded,
		);

		expect(merged).toContain("@@ -2,8 +2,8 @@ function run() {");
		expect(merged).toContain(" middle-1\n middle-2\n middle-3\n stable");
		expect(merged).not.toContain("@@ -8,2 +8,2 @@ function end() {");
	});
});
