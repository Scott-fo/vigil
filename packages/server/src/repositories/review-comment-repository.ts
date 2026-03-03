import { asc, eq } from "drizzle-orm";
import { Context, Data, Effect, Layer } from "effect";
import { DbError, DbService } from "../db/service.ts";
import { reviewCommentsTable } from "../db/schema.ts";
import { ReviewComment, type ReviewComment as ReviewCommentModel } from "../models/review-comment.ts";

export interface CreateReviewCommentInput {
	readonly id?: string;
	readonly threadId: string;
	readonly author?: string;
	readonly body: string;
}

export class ReviewCommentDecodeError extends Data.TaggedError(
	"ReviewCommentDecodeError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

export class ReviewCommentNotFoundError extends Data.TaggedError(
	"ReviewCommentNotFoundError",
)<{
	readonly id: string;
	readonly message: string;
}> {}

export type ReviewCommentRepositoryError =
	| DbError
	| ReviewCommentDecodeError
	| ReviewCommentNotFoundError;

function makeCommentRow(
	row: unknown,
): Effect.Effect<ReviewCommentModel, ReviewCommentDecodeError> {
	return Effect.try({
		try: () =>
			ReviewComment.make(
				row as Parameters<typeof ReviewComment.make>[0],
			),
		catch: (cause) =>
			new ReviewCommentDecodeError({
				message: "Unable to construct review comment model.",
				cause,
			}),
	});
}

function makeCommentRows(
	rows: unknown,
): Effect.Effect<ReadonlyArray<ReviewCommentModel>, ReviewCommentDecodeError> {
	return Effect.try({
		try: () => {
			if (!Array.isArray(rows)) {
				throw new Error("Expected review comment rows to be an array.");
			}

			return rows.map((row) =>
				ReviewComment.make(
					row as Parameters<typeof ReviewComment.make>[0],
				),
			);
		},
		catch: (cause) =>
			new ReviewCommentDecodeError({
				message: "Unable to construct review comment model.",
				cause,
			}),
	});
}

function normalizeId(id: string | undefined): string {
	const trimmed = id?.trim();
	return trimmed && trimmed.length > 0 ? trimmed : crypto.randomUUID();
}

export class ReviewCommentRepository extends Context.Tag(
	"@vigil/server/ReviewCommentRepository",
)<
	ReviewCommentRepository,
		{
			readonly create: (
				input: CreateReviewCommentInput,
			) => Effect.Effect<ReviewCommentModel, ReviewCommentRepositoryError>;
			readonly getById: (
				id: string,
			) => Effect.Effect<ReviewCommentModel, ReviewCommentRepositoryError>;
			readonly listByThreadId: (
				threadId: string,
			) => Effect.Effect<
				ReadonlyArray<ReviewCommentModel>,
				ReviewCommentRepositoryError
			>;
		}
>() {
	static readonly layer = Layer.effect(
		ReviewCommentRepository,
		Effect.gen(function* () {
			const db = yield* DbService;

			const getById = Effect.fn("ReviewCommentRepository.getById")(function* (
				id: string,
			) {
				const row = yield* db.use(
					(client) =>
						client
							.select()
							.from(reviewCommentsTable)
							.where(eq(reviewCommentsTable.id, id))
							.get(),
					"reviewComment.getById",
				);

				if (!row) {
					return yield* new ReviewCommentNotFoundError({
						id,
						message: `Review comment ${id} was not found.`,
					});
				}

				return yield* makeCommentRow(row);
			});

			const create = Effect.fn("ReviewCommentRepository.create")(function* (
				input: CreateReviewCommentInput,
			) {
				const id = normalizeId(input.id);
				const now = Date.now();

				yield* db.use(
					(client) =>
						client
							.insert(reviewCommentsTable)
							.values({
								id,
								threadId: input.threadId,
								author: input.author?.trim() || "local",
								body: input.body,
								createdAtMs: now,
								updatedAtMs: now,
							})
							.run(),
					"reviewComment.create.insert",
				);

				return yield* getById(id);
			});

			const listByThreadId = Effect.fn("ReviewCommentRepository.listByThreadId")(
				function* (threadId: string) {
					const rows = yield* db.use(
						(client) =>
							client
								.select()
								.from(reviewCommentsTable)
								.where(eq(reviewCommentsTable.threadId, threadId))
								.orderBy(asc(reviewCommentsTable.createdAtMs))
								.all(),
						"reviewComment.listByThreadId",
					);

					return yield* makeCommentRows(rows);
				},
			);

			return ReviewCommentRepository.of({
				create,
				getById,
				listByThreadId,
			});
		}),
	);
}
