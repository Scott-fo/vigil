import { describe, expect, test } from "bun:test";
import { searchBranchRefs } from "#ui/branch-ref-search.ts";

describe("searchBranchRefs", () => {
	test("returns refs unchanged when query is blank", () => {
		const refs = ["main", "feature/login-ui", "release/2026.03"];
		expect(searchBranchRefs(refs, "")).toEqual(refs);
		expect(searchBranchRefs(refs, "   ")).toEqual(refs);
	});

	test("sorts refs by fuzzy relevance", () => {
		const refs = [
			"feature/login-ui",
			"fix/login-crash",
			"main",
			"release/2026.03",
		];
		expect(searchBranchRefs(refs, "fl")).toEqual([
			"fix/login-crash",
			"feature/login-ui",
		]);
	});

	test("matches regardless of query case", () => {
		const refs = ["main", "release/2026.03", "feature/auth"];
		expect(searchBranchRefs(refs, "MAIN")).toEqual(["main"]);
	});
});
