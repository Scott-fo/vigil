import { describe, expect, test } from "bun:test";
import { buildDiffNavigationModel } from "#diff/navigation";

describe("buildDiffNavigationModel", () => {
	test("returns empty navigation for diffs without hunks", () => {
		const model = buildDiffNavigationModel(
			["diff --git a/readme.md b/readme.md", "--- a/readme.md", "+++ b/readme.md"].join(
				"\n",
			),
		);

		expect(model.lines).toEqual([]);
		expect(model.hunks).toEqual([]);
	});

	test("maps lines and hunk ranges for unified hunks", () => {
		const model = buildDiffNavigationModel(
			[
				"diff --git a/src/app.ts b/src/app.ts",
				"--- a/src/app.ts",
				"+++ b/src/app.ts",
				"@@ -2,3 +2,4 @@ function run() {",
				" context",
				"-old line",
				"+new line",
				"+another add",
				"@@ -10,2 +11,2 @@ end()",
				"-bye",
				"+ciao",
				" trailing",
			].join("\n"),
		);

		expect(model.lines.map((line) => line.kind)).toEqual([
			"context",
			"remove",
			"add",
			"add",
			"remove",
			"add",
			"context",
		]);

		expect(model.lines[0]).toMatchObject({
			displayIndex: 0,
			hunkIndex: 0,
			oldLine: 2,
			newLine: 2,
			content: "context",
		});
		expect(model.lines[1]).toMatchObject({
			displayIndex: 1,
			hunkIndex: 0,
			oldLine: 3,
			newLine: null,
			content: "old line",
		});
		expect(model.lines[3]).toMatchObject({
			displayIndex: 3,
			hunkIndex: 0,
			oldLine: null,
			newLine: 4,
			content: "another add",
		});
		expect(model.lines[6]).toMatchObject({
			displayIndex: 6,
			hunkIndex: 1,
			oldLine: 11,
			newLine: 12,
			content: "trailing",
		});

		expect(model.hunks).toEqual([
			{
				hunkIndex: 0,
				header: "@@ -2,3 +2,4 @@",
				oldStart: 2,
				oldCount: 3,
				newStart: 2,
				newCount: 4,
				startDisplayIndex: 0,
				endDisplayIndex: 3,
			},
			{
				hunkIndex: 1,
				header: "@@ -10,2 +11,2 @@",
				oldStart: 10,
				oldCount: 2,
				newStart: 11,
				newCount: 2,
				startDisplayIndex: 4,
				endDisplayIndex: 6,
			},
		]);
	});

	test("ignores no-newline metadata rows", () => {
		const model = buildDiffNavigationModel(
			[
				"@@ -1,1 +1,1 @@",
				"-old",
				"\\ No newline at end of file",
				"+new",
			].join("\n"),
		);

		expect(model.lines).toHaveLength(2);
		expect(model.lines[0]?.kind).toBe("remove");
		expect(model.lines[1]?.kind).toBe("add");
		expect(model.hunks[0]).toMatchObject({
			startDisplayIndex: 0,
			endDisplayIndex: 1,
		});
	});
});
