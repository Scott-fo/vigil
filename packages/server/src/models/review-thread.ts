import { Schema } from "effect";

export const ReviewScopeTypeSchema = Schema.Literal(
	"working-tree",
	"branch-compare",
);
export type ReviewScopeType = typeof ReviewScopeTypeSchema.Type;

export const ReviewLineSideSchema = Schema.Literal("old", "new");
export type ReviewLineSide = typeof ReviewLineSideSchema.Type;

export class ReviewThread extends Schema.Class<ReviewThread>("ReviewThread")({
	id: Schema.String,
	repoRoot: Schema.String,
	scopeType: ReviewScopeTypeSchema,
	scopeKey: Schema.String,
	sourceRef: Schema.NullOr(Schema.String),
	destinationRef: Schema.NullOr(Schema.String),
	filePath: Schema.NullOr(Schema.String),
	lineSide: Schema.NullOr(ReviewLineSideSchema),
	lineNumber: Schema.NullOr(Schema.Number),
	hunkHeader: Schema.NullOr(Schema.String),
	lineContentHash: Schema.NullOr(Schema.String),
	isResolved: Schema.Boolean,
	createdAtMs: Schema.Number,
	updatedAtMs: Schema.Number,
}) {}
