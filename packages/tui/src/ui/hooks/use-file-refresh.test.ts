import { describe, expect, test } from "bun:test";
import { Effect, Option } from "effect";
import { FileEntry } from "#tui/types";
import {
	buildFilesLoadEffect,
	consumeQueuedRefresh,
	registerRefreshRequest,
	type RefreshRequestState,
} from "#ui/hooks/use-file-refresh";
import type { ReviewMode } from "#ui/state";

function entry(path: string): FileEntry {
	return FileEntry.make({
		status: "M ",
		path,
		label: path,
	});
}

describe("registerRefreshRequest", () => {
	test("runs immediately when not refreshing", () => {
		const state: RefreshRequestState = {
			isRefreshing: false,
			hasQueuedRefresh: false,
			queuedShowLoading: false,
		};
		const result = registerRefreshRequest(state, false);
		expect(result.shouldRunNow).toBe(true);
		expect(result.nextState).toEqual(state);
	});

	test("queues refresh and preserves showLoading intent while refreshing", () => {
		const state: RefreshRequestState = {
			isRefreshing: true,
			hasQueuedRefresh: false,
			queuedShowLoading: false,
		};
		const first = registerRefreshRequest(state, false);
		const second = registerRefreshRequest(first.nextState, true);

		expect(first.shouldRunNow).toBe(false);
		expect(second.shouldRunNow).toBe(false);
		expect(second.nextState.hasQueuedRefresh).toBe(true);
		expect(second.nextState.queuedShowLoading).toBe(true);
	});
});

describe("consumeQueuedRefresh", () => {
	test("returns none when queue is empty", () => {
		const state: RefreshRequestState = {
			isRefreshing: false,
			hasQueuedRefresh: false,
			queuedShowLoading: false,
		};
		const result = consumeQueuedRefresh(state);
		expect(Option.isNone(result.queuedShowLoading)).toBe(true);
		expect(result.nextState).toEqual(state);
	});

	test("consumes queued refresh and resets queue flags", () => {
		const state: RefreshRequestState = {
			isRefreshing: false,
			hasQueuedRefresh: true,
			queuedShowLoading: true,
		};
		const result = consumeQueuedRefresh(state);
		expect(result.queuedShowLoading).toEqual(Option.some(true));
		expect(result.nextState.hasQueuedRefresh).toBe(false);
		expect(result.nextState.queuedShowLoading).toBe(false);
	});
});

describe("buildFilesLoadEffect", () => {
	test("uses working tree loader in working-tree mode", () => {
		let workingTreeCalls = 0;
		let branchCalls = 0;
		const mode: ReviewMode = { _tag: "working-tree" };
		const result = Effect.runSync(
			buildFilesLoadEffect(mode, {
				loadWorkingTree: () => {
					workingTreeCalls += 1;
					return Effect.succeed([entry("working.ts")]);
				},
				loadBranchCompare: () => {
					branchCalls += 1;
					return Effect.succeed([entry("branch.ts")]);
				},
			}),
		);
		expect(result.map((file) => file.path)).toEqual(["working.ts"]);
		expect(workingTreeCalls).toBe(1);
		expect(branchCalls).toBe(0);
	});

	test("uses branch loader with selected refs in branch-compare mode", () => {
		let capturedSource = "";
		let capturedDestination = "";
		const mode: ReviewMode = {
			_tag: "branch-compare",
			selection: {
				sourceRef: "feature/refactor",
				destinationRef: "main",
			},
		};
		const result = Effect.runSync(
			buildFilesLoadEffect(mode, {
				loadWorkingTree: () => Effect.succeed([entry("working.ts")]),
				loadBranchCompare: (selection) => {
					capturedSource = selection.sourceRef;
					capturedDestination = selection.destinationRef;
					return Effect.succeed([entry("branch.ts")]);
				},
			}),
		);
		expect(result.map((file) => file.path)).toEqual(["branch.ts"]);
		expect(capturedSource).toBe("feature/refactor");
		expect(capturedDestination).toBe("main");
	});
});
