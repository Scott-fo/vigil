import { describe, expect, test } from "bun:test";
import {
	parseDiffNameStatusEntries,
	parseStatusEntries,
	toFileEntry,
} from "#data/git/parsers.ts";

describe("parseStatusEntries", () => {
	test("parses tracked, untracked, and rename entries", () => {
		const raw = " M src/app.ts\0?? README.md\0R  src/old.ts\0src/new.ts\0";
		const entries = parseStatusEntries(raw);

		expect(entries).toEqual([
			{ status: " M", path: "src/app.ts" },
			{ status: "??", path: "README.md" },
			{
				status: "R ",
				path: "src/new.ts",
				originalPath: "src/old.ts",
			},
		]);
	});
});

describe("parseDiffNameStatusEntries", () => {
	test("parses inline and nul-separated diff status entries", () => {
		const raw =
			"M\tsrc/app.ts\0A\0src/new.ts\0R100\tsrc/old.ts\0src/new-name.ts\0";
		const entries = parseDiffNameStatusEntries(raw);

		expect(entries).toEqual([
			{ status: "M ", path: "src/app.ts" },
			{ status: "A ", path: "src/new.ts" },
			{
				status: "R ",
				path: "src/new-name.ts",
				originalPath: "src/old.ts",
			},
		]);
	});
});

describe("toFileEntry", () => {
	test("builds rename labels from parsed entries", () => {
		const file = toFileEntry({
			status: "R ",
			path: "src/new-name.ts",
			originalPath: "src/old.ts",
		});

		expect(file.status).toBe("R ");
		expect(file.path).toBe("src/new-name.ts");
		expect(file.label).toBe("src/old.ts -> src/new-name.ts");
	});
});
