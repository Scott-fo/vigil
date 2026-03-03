import type * as Sse from "@effect/experimental/Sse";
import { Data, Effect, Option, Schema } from "effect";

export interface RepoChangedEvent {
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
	readonly changedAt: string;
}

const RepoChangedEventSchema = Schema.Struct({
	subscriptionId: Schema.String,
	repoRoot: Schema.String,
	version: Schema.Number,
	changedAt: Schema.String,
});

const decodeRepoChangedEvent = Schema.decodeUnknown(RepoChangedEventSchema);
const decodeJsonString = Schema.decodeUnknown(Schema.parseJson());

export class RepoChangedEventDataMissingError extends Data.TaggedError(
	"RepoChangedEventDataMissingError",
)<{
	readonly message: string;
	readonly event: Sse.Event;
}> {}

export class RepoChangedEventJsonParseError extends Data.TaggedError(
	"RepoChangedEventJsonParseError",
)<{
	readonly message: string;
	readonly event: Sse.Event;
	readonly cause: unknown;
}> {}

export class RepoChangedEventPayloadDecodeError extends Data.TaggedError(
	"RepoChangedEventPayloadDecodeError",
)<{
	readonly message: string;
	readonly event: Sse.Event;
	readonly cause: unknown;
}> {}

export type ParseRepoChangedSseEventError =
	| RepoChangedEventDataMissingError
	| RepoChangedEventJsonParseError
	| RepoChangedEventPayloadDecodeError;

export const parseRepoChangedSseEvent = Effect.fn(
	"watchEvents.parseRepoChangedSseEvent",
)(function* (
	event: Sse.Event,
) {
	if (event.event !== "repo-changed") {
		return Option.none();
	}

	if (event.data.length === 0) {
		return yield* new RepoChangedEventDataMissingError({
			message: "repo-changed event did not include a data payload.",
			event,
		});
	}

	const decoded = yield* decodeJsonString(event.data).pipe(
		Effect.mapError(
			(cause) =>
				new RepoChangedEventJsonParseError({
					message: "repo-changed event payload is not valid JSON.",
					event,
					cause,
				}),
		),
	);

	const repoChangedEvent = yield* decodeRepoChangedEvent(decoded).pipe(
		Effect.mapError(
			(cause) =>
				new RepoChangedEventPayloadDecodeError({
					message: "repo-changed event payload does not match expected shape.",
					event,
					cause,
				}),
		),
	);

	return Option.some(repoChangedEvent);
});
