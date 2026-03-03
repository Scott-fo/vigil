import { Data, Effect, Option, Schema } from "effect";

export interface RepoChangedEvent {
	readonly subscriptionId: string;
	readonly repoRoot: string;
	readonly version: number;
	readonly changedAt: string;
}

interface DrainedSseBlocks {
	readonly blocks: ReadonlyArray<string>;
	readonly remaining: string;
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
	readonly block: string;
}> {}

export class RepoChangedEventJsonParseError extends Data.TaggedError(
	"RepoChangedEventJsonParseError",
)<{
	readonly message: string;
	readonly block: string;
	readonly cause: unknown;
}> {}

export class RepoChangedEventPayloadDecodeError extends Data.TaggedError(
	"RepoChangedEventPayloadDecodeError",
)<{
	readonly message: string;
	readonly block: string;
	readonly cause: unknown;
}> {}

export type ParseRepoChangedEventBlockError =
	| RepoChangedEventDataMissingError
	| RepoChangedEventJsonParseError
	| RepoChangedEventPayloadDecodeError;

export function appendSseChunk(buffer: string, chunk: string): string {
	return `${buffer}${chunk.replace(/\r\n/g, "\n")}`;
}

export function drainSseBlocks(buffer: string): DrainedSseBlocks {
	const blocks: Array<string> = [];
	let remaining = buffer;

	while (true) {
		const separatorIndex = remaining.indexOf("\n\n");
		if (separatorIndex < 0) {
			break;
		}

		blocks.push(remaining.slice(0, separatorIndex));
		remaining = remaining.slice(separatorIndex + 2);
	}

	return { blocks, remaining };
}

export const parseRepoChangedEventBlock = Effect.fn(
	"watchEvents.parseRepoChangedEventBlock",
)(function* (
	block: string,
) {
	let eventName = "message";
	const dataLines: Array<string> = [];

	for (const rawLine of block.split("\n")) {
		const line = rawLine.trimEnd();
		if (!line || line.startsWith(":")) {
			continue;
		}

		if (line.startsWith("event:")) {
			eventName = line.slice("event:".length).trim();
			continue;
		}

		if (line.startsWith("data:")) {
			dataLines.push(line.slice("data:".length).trimStart());
		}
	}

	if (eventName !== "repo-changed") {
		return Option.none();
	}

	if (dataLines.length === 0) {
		return yield* new RepoChangedEventDataMissingError({
			message: "repo-changed event did not include a data payload.",
			block,
		});
	}

	const decoded = yield* decodeJsonString(dataLines.join("\n")).pipe(
		Effect.mapError(
			(cause) =>
				new RepoChangedEventJsonParseError({
					message: "repo-changed event payload is not valid JSON.",
					block,
					cause,
				}),
		),
	);

	const event = yield* decodeRepoChangedEvent(decoded).pipe(
		Effect.mapError(
			(cause) =>
				new RepoChangedEventPayloadDecodeError({
					message: "repo-changed event payload does not match expected shape.",
					block,
					cause,
				}),
		),
	);

	return Option.some(event);
});
