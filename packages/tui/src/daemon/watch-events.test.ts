import { describe, expect, test } from "bun:test";
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
		const event = parseRepoChangedEventBlock(
			[
				"event: repo-changed",
				'data: {"subscriptionId":"sub-1","repoRoot":"/tmp/repo","version":3,"changedAt":"2026-03-03T01:02:03.000Z"}',
			].join("\n"),
		);

		expect(event).toEqual({
			subscriptionId: "sub-1",
			repoRoot: "/tmp/repo",
			version: 3,
			changedAt: "2026-03-03T01:02:03.000Z",
		});
	});

	test("ignores comment and non repo-changed blocks", () => {
		expect(parseRepoChangedEventBlock(": connected")).toBeNull();
		expect(
			parseRepoChangedEventBlock(
				["event: keepalive", 'data: {"ok":true}'].join("\n"),
			),
		).toBeNull();
	});

	test("returns null for invalid JSON payloads", () => {
		expect(
			parseRepoChangedEventBlock(
				["event: repo-changed", "data: {not-json}"].join("\n"),
			),
		).toBeNull();
		expect(
			parseRepoChangedEventBlock(
				["event: repo-changed", 'data: {"version":"x"}'].join("\n"),
			),
		).toBeNull();
	});
});
