import { describe, expect, test } from "bun:test";
import { Effect, Either, Option } from "effect";
import {
	appendSseChunk,
	drainSseBlocks,
	parseRepoChangedEventBlock,
} from "#daemon/watch-events.ts";

describe("drainSseBlocks", () => {
	test("splits complete SSE blocks and leaves trailing partial block", () => {
		const result = drainSseBlocks(
			": connected\n\n" +
				"event: repo-changed\ndata: {\"version\":1}\n\n" +
				"event: repo-changed\ndata: partial",
		);

		expect(result.blocks).toEqual([
			": connected",
			"event: repo-changed\ndata: {\"version\":1}",
		]);
		expect(result.remaining).toBe("event: repo-changed\ndata: partial");
	});
});

describe("appendSseChunk", () => {
	test("normalizes CRLF to LF", () => {
		const result = appendSseChunk("", "event: repo-changed\r\ndata: {}\r\n\r\n");
		expect(result).toBe("event: repo-changed\ndata: {}\n\n");
	});
});

describe("parseRepoChangedEventBlock", () => {
	test("parses a valid repo-changed block", () => {
		const event = Effect.runSync(
			parseRepoChangedEventBlock(
				[
					"event: repo-changed",
					'data: {"subscriptionId":"sub-1","repoRoot":"/tmp/repo","version":3,"changedAt":"2026-03-03T01:02:03.000Z"}',
				].join("\n"),
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
		expect(Effect.runSync(parseRepoChangedEventBlock(": connected"))).toEqual(
			Option.none(),
		);
		expect(
			Effect.runSync(
				parseRepoChangedEventBlock(
					["event: keepalive", 'data: {"ok":true}'].join("\n"),
				),
			),
		).toEqual(Option.none());
	});

	test("returns tagged errors for invalid payloads", () => {
		const invalidJson = Effect.runSync(
			parseRepoChangedEventBlock(
				["event: repo-changed", "data: {not-json}"].join("\n"),
			).pipe(Effect.either),
		);
		expect(Either.isLeft(invalidJson)).toBe(true);
		if (Either.isLeft(invalidJson)) {
			expect(invalidJson.left._tag).toBe("RepoChangedEventJsonParseError");
		}

		const invalidShape = Effect.runSync(
			parseRepoChangedEventBlock(
				["event: repo-changed", 'data: {"version":"x"}'].join("\n"),
			).pipe(Effect.either),
		);
		expect(Either.isLeft(invalidShape)).toBe(true);
		if (Either.isLeft(invalidShape)) {
			expect(invalidShape.left._tag).toBe("RepoChangedEventPayloadDecodeError");
		}
	});
});
