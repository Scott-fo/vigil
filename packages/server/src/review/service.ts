import { Context, Effect, Layer, Option, Schema } from "effect";
import type { ReviewThread as ReviewThreadModel } from "../models/review-thread.ts";
import {
	ReviewCommentRepository,
	type ReviewCommentRepositoryError,
	ReviewThreadRepository,
	type ReviewThreadRepositoryError,
} from "../repositories/index.ts";
import {
	buildThreadAnchorKey,
	ReviewScopeValidationError,
} from "./scope.ts";
import type {
	LineThreadAnchor,
	ReviewScope,
	ThreadWithComments,
} from "./types.ts";

export interface CreateOverallThreadInput {
	readonly scope: ReviewScope;
	readonly body: string;
	readonly author?: string;
	readonly threadId?: string;
	readonly commentId?: string;
}

export interface CreateLineThreadInput {
	readonly scope: ReviewScope;
	readonly anchor: LineThreadAnchor;
	readonly body: string;
	readonly author?: string;
	readonly threadId?: string;
	readonly commentId?: string;
}

export interface ReplyToThreadInput {
	readonly scope: ReviewScope;
	readonly threadId: string;
	readonly body: string;
	readonly author?: string;
	readonly commentId?: string;
}

export interface UpdateThreadStateInput {
	readonly scope: ReviewScope;
	readonly threadId: string;
}

export class ReviewServiceValidationError extends Schema.TaggedError<ReviewServiceValidationError>()(
	"ReviewServiceValidationError",
	{
		field: Schema.String,
		message: Schema.String,
	},
) {}

export class ReviewServiceScopeMismatchError extends Schema.TaggedError<ReviewServiceScopeMismatchError>()(
	"ReviewServiceScopeMismatchError",
	{
		threadId: Schema.String,
		message: Schema.String,
	},
) {}

export type ReviewServiceError =
	| ReviewCommentRepositoryError
	| ReviewScopeValidationError
	| ReviewServiceScopeMismatchError
	| ReviewServiceValidationError
	| ReviewThreadRepositoryError;

const validateNonEmpty = Effect.fn("ReviewService.validateNonEmpty")(function* (
	field: string,
	value: string,
) {
	const normalized = value.trim();

	if (normalized.length === 0) {
		return yield* ReviewServiceValidationError.make({
			field,
			message: `${field} must not be empty.`,
		});
	}

	return normalized;
});

const validateLineNumber = Effect.fn("ReviewService.validateLineNumber")(function* (
	value: number,
) {
	if (!Number.isInteger(value) || value < 1) {
		return yield* ReviewServiceValidationError.make({
			field: "lineNumber",
			message: "lineNumber must be an integer greater than or equal to 1.",
		});
	}

	return value;
});

function optionToNullable(value: Option.Option<string>): string | null {
	return Option.match(value, {
		onNone: () => null,
		onSome: (v) => v,
	});
}

function inScope(scope: ReviewScope, thread: ReviewThreadModel): boolean {
	return (
		thread.repoRoot === scope.repoRoot &&
		thread.scopeKey === scope.scopeKey &&
		thread.scopeType === scope.mode
	);
}

export class ReviewService extends Context.Tag("@vigil/server/ReviewService")<
	ReviewService,
	{
		readonly createOverallThread: (
			input: CreateOverallThreadInput,
		) => Effect.Effect<ThreadWithComments, ReviewServiceError>;
		readonly createLineThread: (
			input: CreateLineThreadInput,
		) => Effect.Effect<ThreadWithComments, ReviewServiceError>;
		readonly replyToThread: (
			input: ReplyToThreadInput,
		) => Effect.Effect<ThreadWithComments, ReviewServiceError>;
		readonly resolveThread: (
			input: UpdateThreadStateInput,
		) => Effect.Effect<ReviewThreadModel, ReviewServiceError>;
		readonly reopenThread: (
			input: UpdateThreadStateInput,
		) => Effect.Effect<ReviewThreadModel, ReviewServiceError>;
	}
>() {
	static readonly layer = Layer.effect(
		ReviewService,
		Effect.gen(function* () {
			const threadRepository = yield* ReviewThreadRepository;
			const commentRepository = yield* ReviewCommentRepository;

			const normalizeScope = Effect.fn("ReviewService.normalizeScope")(function* (
				scope: ReviewScope,
			) {
				const repoRoot = yield* validateNonEmpty("repoRoot", scope.repoRoot);
				const scopeKey = yield* validateNonEmpty("scopeKey", scope.scopeKey);

				return {
					...scope,
					repoRoot,
					scopeKey,
				} satisfies ReviewScope;
			});

			const getThreadInScope = Effect.fn("ReviewService.getThreadInScope")(
				function* (scope: ReviewScope, threadId: string) {
					const normalizedThreadId = yield* validateNonEmpty("threadId", threadId);
					const thread = yield* threadRepository.getById(normalizedThreadId);

					if (!inScope(scope, thread)) {
						return yield* ReviewServiceScopeMismatchError.make({
							threadId: thread.id,
							message:
								"Review thread does not belong to the requested repository scope.",
						});
					}

					return thread;
				},
			);

			const createThreadWithComment = Effect.fn(
				"ReviewService.createThreadWithComment",
			)(
				function* (options: {
					readonly threadInput: Parameters<
						typeof threadRepository.create
					>[0];
					readonly body: string;
					readonly author?: string;
					readonly commentId?: string;
				}) {
					const thread = yield* threadRepository.create(options.threadInput);
					const commentInput = {
						threadId: thread.id,
						body: options.body,
						...(options.commentId === undefined ? {} : { id: options.commentId }),
						...(options.author === undefined ? {} : { author: options.author }),
					};
					const comment = yield* commentRepository.create(commentInput);

					return {
						thread,
						comments: [comment],
						isStale: false,
					} satisfies ThreadWithComments;
				},
			);

			const createOverallThread = Effect.fn(
				"ReviewService.createOverallThread",
			)(function* (input: CreateOverallThreadInput) {
				const scope = yield* normalizeScope(input.scope);
				const body = yield* validateNonEmpty("body", input.body);
				const threadInput = {
					repoRoot: scope.repoRoot,
					scopeType: scope.mode,
					scopeKey: scope.scopeKey,
					sourceRef: optionToNullable(scope.sourceRef),
					destinationRef: optionToNullable(scope.destinationRef),
					filePath: null,
					lineSide: null,
					lineNumber: null,
					hunkHeader: null,
					lineContentHash: null,
					isResolved: false,
					...(input.threadId === undefined ? {} : { id: input.threadId }),
				};

				return yield* createThreadWithComment({
					threadInput,
					body,
					...(input.commentId === undefined
						? {}
						: { commentId: input.commentId }),
					...(input.author === undefined ? {} : { author: input.author }),
				});
			});

			const createLineThread = Effect.fn("ReviewService.createLineThread")(
				function* (input: CreateLineThreadInput) {
					const scope = yield* normalizeScope(input.scope);
					const body = yield* validateNonEmpty("body", input.body);
					const filePath = yield* validateNonEmpty(
						"filePath",
						input.anchor.filePath,
					);
					const lineNumber = yield* validateLineNumber(input.anchor.lineNumber);

					const anchor: LineThreadAnchor = {
						...input.anchor,
						filePath,
						lineNumber,
					};
					yield* buildThreadAnchorKey(anchor);
					const threadInput = {
						repoRoot: scope.repoRoot,
						scopeType: scope.mode,
						scopeKey: scope.scopeKey,
						sourceRef: optionToNullable(scope.sourceRef),
						destinationRef: optionToNullable(scope.destinationRef),
						filePath,
						lineSide: anchor.lineSide,
						lineNumber,
						hunkHeader: optionToNullable(anchor.hunkHeader),
						lineContentHash: optionToNullable(anchor.lineContentHash),
						isResolved: false,
						...(input.threadId === undefined ? {} : { id: input.threadId }),
					};

					return yield* createThreadWithComment({
						threadInput,
						body,
						...(input.commentId === undefined
							? {}
							: { commentId: input.commentId }),
						...(input.author === undefined ? {} : { author: input.author }),
					});
				},
			);

			const replyToThread = Effect.fn("ReviewService.replyToThread")(function* (
				input: ReplyToThreadInput,
			) {
				const scope = yield* normalizeScope(input.scope);
				const body = yield* validateNonEmpty("body", input.body);
				const thread = yield* getThreadInScope(scope, input.threadId);

				const commentInput = {
					threadId: thread.id,
					body,
					...(input.commentId === undefined ? {} : { id: input.commentId }),
					...(input.author === undefined ? {} : { author: input.author }),
				};
				yield* commentRepository.create(commentInput);

				const comments = yield* commentRepository.listByThreadId(thread.id);

				return {
					thread,
					comments,
					isStale: false,
				} satisfies ThreadWithComments;
			});

			const updateThreadState = Effect.fn("ReviewService.updateThreadState")(
				function* (input: UpdateThreadStateInput, isResolved: boolean) {
					const scope = yield* normalizeScope(input.scope);
					const thread = yield* getThreadInScope(scope, input.threadId);
					return yield* threadRepository.setResolved(thread.id, isResolved);
				},
			);

			const resolveThread = Effect.fn("ReviewService.resolveThread")(function* (
				input: UpdateThreadStateInput,
			) {
				return yield* updateThreadState(input, true);
			});

			const reopenThread = Effect.fn("ReviewService.reopenThread")(function* (
				input: UpdateThreadStateInput,
			) {
				return yield* updateThreadState(input, false);
			});

			return ReviewService.of({
				createOverallThread,
				createLineThread,
				replyToThread,
				resolveThread,
				reopenThread,
			});
		}),
	);
}
