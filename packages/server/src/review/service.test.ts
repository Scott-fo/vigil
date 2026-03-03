import { describe, expect, test } from "bun:test";
import { Cause, Effect, Exit, Layer, Option } from "effect";
import { ReviewComment } from "../models/review-comment.ts";
import { ReviewThread } from "../models/review-thread.ts";
import { ReviewCommentRepository } from "../repositories/review-comment-repository.ts";
import { ReviewThreadRepository } from "../repositories/review-thread-repository.ts";
import { createWorkingTreeScope } from "./scope.ts";
import { ReviewService } from "./service.ts";

const repoRoot = "/repo";
const scopeKey = "working-tree:main";

const overallThread = ReviewThread.make({
	id: "thread-overall",
	repoRoot,
	scopeType: "working-tree",
	scopeKey,
	sourceRef: null,
	destinationRef: null,
	filePath: null,
	lineSide: null,
	lineNumber: null,
	hunkHeader: null,
	lineContentHash: null,
	isResolved: false,
	createdAtMs: 1,
	updatedAtMs: 1,
});

const freshLineThread = ReviewThread.make({
	id: "thread-fresh",
	repoRoot,
	scopeType: "working-tree",
	scopeKey,
	sourceRef: null,
	destinationRef: null,
	filePath: "src/file.ts",
	lineSide: "new",
	lineNumber: 42,
	hunkHeader: null,
	lineContentHash: null,
	isResolved: false,
	createdAtMs: 2,
	updatedAtMs: 2,
});

const staleLineThread = ReviewThread.make({
	id: "thread-stale",
	repoRoot,
	scopeType: "working-tree",
	scopeKey,
	sourceRef: null,
	destinationRef: null,
	filePath: "src/file.ts",
	lineSide: "new",
	lineNumber: 88,
	hunkHeader: null,
	lineContentHash: null,
	isResolved: false,
	createdAtMs: 3,
	updatedAtMs: 3,
});

const malformedLineThread = ReviewThread.make({
	id: "thread-malformed",
	repoRoot,
	scopeType: "working-tree",
	scopeKey,
	sourceRef: null,
	destinationRef: null,
	filePath: "src/file.ts",
	lineSide: null,
	lineNumber: 17,
	hunkHeader: null,
	lineContentHash: null,
	isResolved: false,
	createdAtMs: 4,
	updatedAtMs: 4,
});

const commentByThreadId = {
	[overallThread.id]: [
		ReviewComment.make({
			id: "comment-overall",
			threadId: overallThread.id,
			author: "local",
			body: "overall",
			createdAtMs: 10,
			updatedAtMs: 10,
		}),
	],
	[freshLineThread.id]: [
		ReviewComment.make({
			id: "comment-fresh",
			threadId: freshLineThread.id,
			author: "local",
			body: "fresh",
			createdAtMs: 11,
			updatedAtMs: 11,
		}),
	],
	[staleLineThread.id]: [
		ReviewComment.make({
			id: "comment-stale",
			threadId: staleLineThread.id,
			author: "local",
			body: "stale",
			createdAtMs: 12,
			updatedAtMs: 12,
		}),
	],
	[malformedLineThread.id]: [
		ReviewComment.make({
			id: "comment-malformed",
			threadId: malformedLineThread.id,
			author: "local",
			body: "malformed",
			createdAtMs: 13,
			updatedAtMs: 13,
		}),
	],
} satisfies Record<string, ReadonlyArray<ReviewComment>>;

function makeReviewServiceLayer(threads: ReadonlyArray<ReviewThread>) {
	const threadRepositoryLayer = Layer.succeed(
		ReviewThreadRepository,
		ReviewThreadRepository.of({
			create: () => Effect.die("create should not be called in list tests"),
			getById: () => Effect.die("getById should not be called in list tests"),
			listByScope: () => Effect.succeed(threads),
			setResolved: () =>
				Effect.die("setResolved should not be called in list tests"),
		}),
	);

	const commentRepositoryLayer = Layer.succeed(
		ReviewCommentRepository,
		ReviewCommentRepository.of({
			create: () =>
				Effect.die("create should not be called in list tests"),
			getById: () =>
				Effect.die("getById should not be called in list tests"),
			listByThreadId: (threadId) =>
				Effect.succeed(commentByThreadId[threadId] ?? []),
		}),
	);

	return ReviewService.layer.pipe(
		Layer.provide(threadRepositoryLayer),
		Layer.provide(commentRepositoryLayer),
	);
}

describe("ReviewService.listThreads", () => {
	test("hides stale line threads by default when active anchors are provided", () => {
		const scope = Effect.runSync(
			createWorkingTreeScope({
				repoRoot,
				branchOrHead: "main",
			}),
		);

		const layer = makeReviewServiceLayer([
			overallThread,
			freshLineThread,
			staleLineThread,
		]);

		const result = Effect.runSync(
			Effect.gen(function* () {
				const reviewService = yield* ReviewService;
				return yield* reviewService.listThreads({
					scope,
					activeAnchors: [
						{
							anchorType: "line",
							filePath: "src/file.ts",
							lineSide: "new",
							lineNumber: 42,
							hunkHeader: Option.none(),
							lineContentHash: Option.none(),
						},
					],
				});
			}).pipe(Effect.provide(layer)),
		);

		expect(result.map((entry) => entry.thread.id)).toEqual([
			overallThread.id,
			freshLineThread.id,
		]);
		expect(result.every((entry) => entry.isStale === false)).toBe(true);
	});

	test("includeStale returns stale threads with stale flag", () => {
		const scope = Effect.runSync(
			createWorkingTreeScope({
				repoRoot,
				branchOrHead: "main",
			}),
		);

		const layer = makeReviewServiceLayer([
			overallThread,
			freshLineThread,
			staleLineThread,
		]);

		const result = Effect.runSync(
			Effect.gen(function* () {
				const reviewService = yield* ReviewService;
				return yield* reviewService.listThreads({
					scope,
					includeStale: true,
					activeAnchors: [
						{
							anchorType: "line",
							filePath: "src/file.ts",
							lineSide: "new",
							lineNumber: 42,
							hunkHeader: Option.none(),
							lineContentHash: Option.none(),
						},
					],
				});
			}).pipe(Effect.provide(layer)),
		);

		expect(result.map((entry) => [entry.thread.id, entry.isStale])).toEqual([
			[overallThread.id, false],
			[freshLineThread.id, false],
			[staleLineThread.id, true],
		]);
	});

	test("treats malformed persisted line anchors as stale", () => {
		const scope = Effect.runSync(
			createWorkingTreeScope({
				repoRoot,
				branchOrHead: "main",
			}),
		);

		const layer = makeReviewServiceLayer([malformedLineThread]);

		const withoutStale = Effect.runSync(
			Effect.gen(function* () {
				const reviewService = yield* ReviewService;
				return yield* reviewService.listThreads({
					scope,
					activeAnchors: [
						{
							anchorType: "line",
							filePath: "src/file.ts",
							lineSide: "new",
							lineNumber: 17,
							hunkHeader: Option.none(),
							lineContentHash: Option.none(),
						},
					],
				});
			}).pipe(Effect.provide(layer)),
		);
		expect(withoutStale).toEqual([]);

		const withStale = Effect.runSync(
			Effect.gen(function* () {
				const reviewService = yield* ReviewService;
				return yield* reviewService.listThreads({
					scope,
					includeStale: true,
					activeAnchors: [
						{
							anchorType: "line",
							filePath: "src/file.ts",
							lineSide: "new",
							lineNumber: 17,
							hunkHeader: Option.none(),
							lineContentHash: Option.none(),
						},
					],
				});
			}).pipe(Effect.provide(layer)),
		);
		expect(withStale).toHaveLength(1);
		expect(withStale[0]?.thread.id).toBe(malformedLineThread.id);
		expect(withStale[0]?.isStale).toBe(true);
	});

	test("fails with typed validation error for blank filePath filter", () => {
		const scope = Effect.runSync(
			createWorkingTreeScope({
				repoRoot,
				branchOrHead: "main",
			}),
		);

		const layer = makeReviewServiceLayer([overallThread]);

		const exit = Effect.runSyncExit(
			Effect.gen(function* () {
				const reviewService = yield* ReviewService;
				return yield* reviewService.listThreads({
					scope,
					filePath: "   ",
				});
			}).pipe(Effect.provide(layer)),
		);

		expect(Exit.isFailure(exit)).toBe(true);

		if (Exit.isFailure(exit)) {
			const failure = Cause.failureOption(exit.cause);
			expect(Option.isSome(failure)).toBe(true);
			if (Option.isSome(failure)) {
				expect(failure.value._tag).toBe("ReviewServiceValidationError");
				if (failure.value._tag === "ReviewServiceValidationError") {
					expect(failure.value.field).toBe("filePath");
				}
			}
		}
	});
});
