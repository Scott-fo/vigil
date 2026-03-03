import { describe, expect, test } from "bun:test";
import { Cause, Effect, Exit, Option } from "effect";
import {
	buildBranchCompareScopeKey,
	buildThreadAnchorKey,
	buildWorkingTreeScopeKey,
	createBranchCompareScope,
	createOverallAnchor,
	createWorkingTreeScope,
} from "./scope.ts";

describe("review scope keys", () => {
	test("buildWorkingTreeScopeKey trims and formats branch scope", () => {
		expect(Effect.runSync(buildWorkingTreeScopeKey("  feature/refactor  "))).toBe(
			"working-tree:feature/refactor",
		);
	});

	test("buildBranchCompareScopeKey uses destination...source ordering", () => {
		expect(
			Effect.runSync(
				buildBranchCompareScopeKey({
					sourceRef: " feature/refactor ",
					destinationRef: " main ",
				}),
			),
		).toBe("branch-compare:main...feature/refactor");
	});

	test("createWorkingTreeScope sets refs to Option.none", () => {
		expect(
			Effect.runSync(
				createWorkingTreeScope({
					repoRoot: " /repo ",
					branchOrHead: " main ",
				}),
			),
		).toEqual({
			repoRoot: "/repo",
			mode: "working-tree",
			sourceRef: Option.none(),
			destinationRef: Option.none(),
			scopeKey: "working-tree:main",
		});
	});

	test("createBranchCompareScope preserves refs and scope key", () => {
		expect(
			Effect.runSync(
				createBranchCompareScope({
					repoRoot: " /repo ",
					sourceRef: " feature/a ",
					destinationRef: " main ",
				}),
			),
		).toEqual({
			repoRoot: "/repo",
			mode: "branch-compare",
			sourceRef: Option.some("feature/a"),
			destinationRef: Option.some("main"),
			scopeKey: "branch-compare:main...feature/a",
		});
	});

	test("buildWorkingTreeScopeKey fails with typed error for blank branch", () => {
		const exit = Effect.runSyncExit(buildWorkingTreeScopeKey("   "));

		expect(Exit.isFailure(exit)).toBe(true);

		if (Exit.isFailure(exit)) {
			const failure = Cause.failureOption(exit.cause);
			expect(Option.isSome(failure)).toBe(true);
			if (Option.isSome(failure)) {
				expect(failure.value._tag).toBe("ReviewScopeValidationError");
				expect(failure.value.field).toBe("branchOrHead");
			}
		}
	});
});

describe("thread anchor keys", () => {
	test("buildThreadAnchorKey for overall anchor", () => {
		expect(Effect.runSync(buildThreadAnchorKey(createOverallAnchor()))).toBe(
			"overall",
		);
	});

	test("buildThreadAnchorKey for line anchor includes file side line and metadata", () => {
		expect(
			Effect.runSync(
				buildThreadAnchorKey({
					anchorType: "line",
					filePath: "src/repo.ts",
					lineSide: "new",
					lineNumber: 42,
					lineContentHash: Option.some("abc123"),
					hunkHeader: Option.some("@@ -1,1 +1,2 @@"),
				}),
			),
		).toBe("line|src/repo.ts|new|42|abc123|@@ -1,1 +1,2 @@");
	});

	test("buildThreadAnchorKey line anchor defaults optional metadata to empty parts", () => {
		expect(
			Effect.runSync(
				buildThreadAnchorKey({
					anchorType: "line",
					filePath: "src/repo.ts",
					lineSide: "old",
					lineNumber: 7,
					lineContentHash: Option.none(),
					hunkHeader: Option.none(),
				}),
			),
		).toBe("line|src/repo.ts|old|7||");
	});
});
