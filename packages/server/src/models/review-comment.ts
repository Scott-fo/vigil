import { Schema } from "effect";

export class ReviewComment extends Schema.Class<ReviewComment>("ReviewComment")({
	id: Schema.String,
	threadId: Schema.String,
	author: Schema.String,
	body: Schema.String,
	createdAtMs: Schema.Number,
	updatedAtMs: Schema.Number,
}) {}

export const decodeReviewComment = Schema.decodeUnknown(ReviewComment);

export const decodeReviewComments = Schema.decodeUnknown(
	Schema.Array(ReviewComment),
);
