import { describe, expect, test } from "bun:test";
import { Effect, Either, Option } from "effect";
import { parseRepoChangedSseEvent } from "#daemon/watch-events.ts";

const makeSseEvent = (event: string, data: string) => ({
	_tag: "Event" as const,
	event,
	id: undefined,
	data,
});

describe("parseRepoChangedSseEvent", () => {
	test("parses a valid repo-changed block", () => {
		const event = Effect.runSync(
			parseRepoChangedSseEvent(
				makeSseEvent(
					"repo-changed",
					'{"subscriptionId":"sub-1","repoRoot":"/tmp/repo","version":3,"changedAt":"2026-03-03T01:02:03.000Z"}',
				),
			),
		);

		expect(event).toEqual(
			Option.some({
				subscriptionId: "sub-1",
				repoRoot: "/tmp/repo",
				version: 3,
				changedAt: "2026-03-03T01:02:03.000Z",
			}),
		);
	});

	test("ignores comment and non repo-changed blocks", () => {
		expect(
			Effect.runSync(parseRepoChangedSseEvent(makeSseEvent("message", "hello"))),
		).toEqual(Option.none());
		expect(
			Effect.runSync(
				parseRepoChangedSseEvent(makeSseEvent("keepalive", '{"ok":true}')),
			),
		).toEqual(
			Option.none(),
		);
	});

	test("returns tagged errors for invalid payloads", () => {
		const missingData = Effect.runSync(
			parseRepoChangedSseEvent(makeSseEvent("repo-changed", "")).pipe(
				Effect.either,
			),
		);
		expect(Either.isLeft(missingData)).toBe(true);
		if (Either.isLeft(missingData)) {
			expect(missingData.left._tag).toBe("RepoChangedEventDataMissingError");
		}

		const invalidJson = Effect.runSync(
			parseRepoChangedSseEvent(
				makeSseEvent("repo-changed", "{not-json}"),
			).pipe(Effect.either),
		);
		expect(Either.isLeft(invalidJson)).toBe(true);
		if (Either.isLeft(invalidJson)) {
			expect(invalidJson.left._tag).toBe("RepoChangedEventJsonParseError");
		}

		const invalidShape = Effect.runSync(
			parseRepoChangedSseEvent(
				makeSseEvent("repo-changed", '{"version":"x"}'),
			).pipe(Effect.either),
		);
		expect(Either.isLeft(invalidShape)).toBe(true);
		if (Either.isLeft(invalidShape)) {
			expect(invalidShape.left._tag).toBe("RepoChangedEventPayloadDecodeError");
		}
	});
});
