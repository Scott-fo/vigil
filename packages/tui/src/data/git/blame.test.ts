import { describe, expect, test } from "bun:test";
import { Option } from "effect";
import {
	isUncommittedBlameHash,
	parseBlamePorcelainHeader,
	parseCommitShowOutput,
} from "#data/git.ts";

describe("parseBlamePorcelainHeader", () => {
	test("parses commit hash, author, date, and summary", () => {
		const raw = [
			"7d6f4a5f9b819195f9f2008fa5806f2f88f5ec3a 12 12 1",
			"author Jane Doe",
			"author-mail <jane@example.com>",
			"author-time 1700000000",
			"author-tz +0000",
			"summary fix parser edge case",
			"filename src/app.ts",
			"\tconst x = 1;",
		].join("\n");

		const parsed = parseBlamePorcelainHeader(raw);
		expect(Option.isSome(parsed)).toBe(true);
		if (Option.isSome(parsed)) {
			expect(parsed.value).toEqual({
				commitHash: "7d6f4a5f9b819195f9f2008fa5806f2f88f5ec3a",
				author: "Jane Doe",
				date: "2023-11-14",
				summary: "fix parser edge case",
			});
		}
	});

	test("returns none for malformed blame output", () => {
		const parsed = parseBlamePorcelainHeader("not a blame header");
		expect(Option.isNone(parsed)).toBe(true);
	});
});

describe("parseCommitShowOutput", () => {
	test("parses show output fields", () => {
		const raw = [
			"7d6f4a5f9b819195f9f2008fa5806f2f88f5ec3a",
			"7d6f4a5",
			"aaaa bbbb",
			"2026-03-05",
			"Jane Doe",
			"feat: wire blame view",
			"Detailed body line 1\nline 2",
		].join("\u001f");

		const parsed = parseCommitShowOutput(raw);
		expect(Option.isSome(parsed)).toBe(true);
		if (Option.isSome(parsed)) {
			expect(parsed.value.commitHash).toBe(
				"7d6f4a5f9b819195f9f2008fa5806f2f88f5ec3a",
			);
			expect(parsed.value.shortHash).toBe("7d6f4a5");
			expect(parsed.value.parentHashes).toEqual(["aaaa", "bbbb"]);
			expect(parsed.value.subject).toBe("feat: wire blame view");
			expect(parsed.value.description).toBe("Detailed body line 1\nline 2");
		}
	});

	test("returns none for missing commit fields", () => {
		const parsed = parseCommitShowOutput("\u001f\u001f\u001f");
		expect(Option.isNone(parsed)).toBe(true);
	});
});

describe("isUncommittedBlameHash", () => {
	test("detects the uncommitted blame sentinel", () => {
		expect(
			isUncommittedBlameHash("0000000000000000000000000000000000000000"),
		).toBe(true);
		expect(
			isUncommittedBlameHash("7d6f4a5f9b819195f9f2008fa5806f2f88f5ec3a"),
		).toBe(false);
	});
});
