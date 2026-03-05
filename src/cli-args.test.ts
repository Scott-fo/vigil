import { describe, expect, test } from "bun:test";
import { Either, Effect, Option } from "effect";
import { parseBlameTarget, parseVigilArgs } from "./cli-args";

describe("parseBlameTarget", () => {
	test("parses <file>:<line>", () => {
		const result = Effect.runSync(Effect.either(parseBlameTarget("src/app.ts:42")));
		expect(Either.isRight(result)).toBe(true);
		if (Either.isRight(result)) {
			expect(result.right).toEqual({
				filePath: "src/app.ts",
				lineNumber: 42,
			});
		}
	});

	test("rejects invalid line numbers", () => {
		const result = Effect.runSync(Effect.either(parseBlameTarget("src/app.ts:0")));
		expect(Either.isLeft(result)).toBe(true);
	});
});

describe("parseVigilArgs", () => {
	test("parses default tui mode", () => {
		const result = Effect.runSync(Effect.either(parseVigilArgs([])));
		expect(Either.isRight(result)).toBe(true);
		if (Either.isRight(result)) {
			expect(result.right.command).toBe("tui");
			expect(Option.isNone(result.right.initialBlameTarget)).toBe(true);
		}
	});

	test("parses blame mode target", () => {
		const result = Effect.runSync(
			Effect.either(parseVigilArgs(["blame", "src/index.tsx:7"])),
		);
		expect(Either.isRight(result)).toBe(true);
		if (Either.isRight(result)) {
			expect(result.right.command).toBe("tui");
			expect(result.right.initialBlameTarget).toEqual(
				Option.some({
					filePath: "src/index.tsx",
					lineNumber: 7,
				}),
			);
		}
	});

	test("rejects malformed blame invocation", () => {
		const missingTarget = Effect.runSync(
			Effect.either(parseVigilArgs(["blame"])),
		);
		expect(Either.isLeft(missingTarget)).toBe(true);

		const extraPositionals = Effect.runSync(
			Effect.either(parseVigilArgs(["blame", "src/a.ts:2", "extra"])),
		);
		expect(Either.isLeft(extraPositionals)).toBe(true);
	});

	test("rejects chooser option in serve mode", () => {
		const result = Effect.runSync(
			Effect.either(parseVigilArgs(["serve", "--chooser-file", "/tmp/x"])),
		);
		expect(Either.isLeft(result)).toBe(true);
	});
});
