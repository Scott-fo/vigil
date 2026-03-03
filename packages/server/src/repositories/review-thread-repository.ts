import { and, desc, eq, isNull } from "drizzle-orm";
import { Context, Data, Effect, Layer } from "effect";
import { DbError, DbService } from "../db/service.ts";
import { reviewThreadsTable } from "../db/schema.ts";
import {
	ReviewThread,
	type ReviewLineSide,
	type ReviewScopeType,
	type ReviewThread as ReviewThreadModel,
} from "../models/review-thread.ts";

export interface CreateReviewThreadInput {
	readonly id?: string;
	readonly repoRoot: string;
	readonly scopeType: ReviewScopeType;
	readonly scopeKey: string;
	readonly sourceRef?: string | null;
	readonly destinationRef?: string | null;
	readonly filePath?: string | null;
	readonly lineSide?: ReviewLineSide | null;
	readonly lineNumber?: number | null;
	readonly hunkHeader?: string | null;
	readonly lineContentHash?: string | null;
	readonly isResolved?: boolean;
}

export interface ListReviewThreadsOptions {
	readonly repoRoot: string;
	readonly scopeKey: string;
	readonly filePath?: string | null;
	readonly includeResolved?: boolean;
}

export class ReviewThreadDecodeError extends Data.TaggedError(
	"ReviewThreadDecodeError",
)<{
	readonly message: string;
	readonly cause: unknown;
}> {}

export class ReviewThreadNotFoundError extends Data.TaggedError(
	"ReviewThreadNotFoundError",
)<{
	readonly id: string;
	readonly message: string;
}> {}

export type ReviewThreadRepositoryError =
	| DbError
	| ReviewThreadDecodeError
	| ReviewThreadNotFoundError;

function makeThreadRow(
	row: unknown,
): Effect.Effect<ReviewThreadModel, ReviewThreadDecodeError> {
	return Effect.try({
		try: () =>
			ReviewThread.make(
				row as Parameters<typeof ReviewThread.make>[0],
			),
		catch: (cause) =>
			new ReviewThreadDecodeError({
				message: "Unable to construct review thread model.",
				cause,
			}),
	});
}

function makeThreadRows(
	rows: unknown,
): Effect.Effect<ReadonlyArray<ReviewThreadModel>, ReviewThreadDecodeError> {
	return Effect.try({
		try: () => {
			if (!Array.isArray(rows)) {
				throw new Error("Expected review thread rows to be an array.");
			}

			return rows.map((row) =>
				ReviewThread.make(
					row as Parameters<typeof ReviewThread.make>[0],
				),
			);
		},
		catch: (cause) =>
			new ReviewThreadDecodeError({
				message: "Unable to construct review thread model.",
				cause,
			}),
	});
}

function normalizeId(id: string | undefined): string {
	const trimmed = id?.trim();
	return trimmed && trimmed.length > 0 ? trimmed : crypto.randomUUID();
}

export class ReviewThreadRepository extends Context.Tag(
	"@vigil/server/ReviewThreadRepository",
)<
	ReviewThreadRepository,
		{
			readonly create: (
				input: CreateReviewThreadInput,
			) => Effect.Effect<ReviewThreadModel, ReviewThreadRepositoryError>;
			readonly getById: (
				id: string,
			) => Effect.Effect<ReviewThreadModel, ReviewThreadRepositoryError>;
			readonly listByScope: (
				options: ListReviewThreadsOptions,
			) => Effect.Effect<
				ReadonlyArray<ReviewThreadModel>,
				ReviewThreadRepositoryError
			>;
			readonly setResolved: (
				id: string,
				isResolved: boolean,
			) => Effect.Effect<ReviewThreadModel, ReviewThreadRepositoryError>;
		}
>() {
	static readonly layer = Layer.effect(
		ReviewThreadRepository,
		Effect.gen(function* () {
			const db = yield* DbService;

			const getById = Effect.fn("ReviewThreadRepository.getById")(function* (
				id: string,
			) {
				const row = yield* db.use(
					(client) =>
						client
							.select()
							.from(reviewThreadsTable)
							.where(eq(reviewThreadsTable.id, id))
							.get(),
					"reviewThread.getById",
				);

				if (!row) {
					return yield* new ReviewThreadNotFoundError({
						id,
						message: `Review thread ${id} was not found.`,
					});
				}

				return yield* makeThreadRow(row);
			});

			const create = Effect.fn("ReviewThreadRepository.create")(function* (
				input: CreateReviewThreadInput,
			) {
				const id = normalizeId(input.id);
				const now = Date.now();

				yield* db.use(
					(client) =>
						client
							.insert(reviewThreadsTable)
							.values({
								id,
								repoRoot: input.repoRoot,
								scopeType: input.scopeType,
								scopeKey: input.scopeKey,
								sourceRef: input.sourceRef ?? null,
								destinationRef: input.destinationRef ?? null,
								filePath: input.filePath ?? null,
								lineSide: input.lineSide ?? null,
								lineNumber: input.lineNumber ?? null,
								hunkHeader: input.hunkHeader ?? null,
								lineContentHash: input.lineContentHash ?? null,
								isResolved: input.isResolved ?? false,
								createdAtMs: now,
								updatedAtMs: now,
							})
							.run(),
					"reviewThread.create.insert",
				);

				return yield* getById(id);
			});

			const listByScope = Effect.fn("ReviewThreadRepository.listByScope")(
				function* (options: ListReviewThreadsOptions) {
					let filter = and(
						eq(reviewThreadsTable.repoRoot, options.repoRoot),
						eq(reviewThreadsTable.scopeKey, options.scopeKey),
					);

					if (options.filePath !== undefined) {
						filter =
							options.filePath === null
								? and(filter, isNull(reviewThreadsTable.filePath))
								: and(filter, eq(reviewThreadsTable.filePath, options.filePath));
					}

					if (!options.includeResolved) {
						filter = and(filter, eq(reviewThreadsTable.isResolved, false));
					}

					const rows = yield* db.use(
						(client) =>
							client
								.select()
								.from(reviewThreadsTable)
								.where(filter)
								.orderBy(
									desc(reviewThreadsTable.updatedAtMs),
									desc(reviewThreadsTable.createdAtMs),
								)
								.all(),
						"reviewThread.listByScope",
					);

					return yield* makeThreadRows(rows);
				},
			);

			const setResolved = Effect.fn("ReviewThreadRepository.setResolved")(
				function* (id: string, isResolved: boolean) {
					yield* db.use(
						(client) =>
							client
								.update(reviewThreadsTable)
								.set({
									isResolved,
									updatedAtMs: Date.now(),
								})
								.where(eq(reviewThreadsTable.id, id))
								.run(),
						"reviewThread.setResolved",
					);

					return yield* getById(id);
				},
			);

			return ReviewThreadRepository.of({
				create,
				getById,
				listByScope,
				setResolved,
			});
		}),
	);
}
