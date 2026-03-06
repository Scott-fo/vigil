import { describe, expect, test } from "bun:test";
import { runWithSuspendedRenderer } from "#ui/hooks/use-git-actions.ts";

function createRenderer() {
	const events: string[] = [];

	return {
		events,
		renderer: {
			height: 24,
			currentRenderBuffer: {
				clear() {
					events.push("clear");
				},
			},
			destroy() {
				events.push("destroy");
			},
			requestRender() {
				events.push("requestRender");
			},
			suspend() {
				events.push("suspend");
			},
			resume() {
				events.push("resume");
			},
		},
	};
}

describe("runWithSuspendedRenderer", () => {
	test("restores and redraws after editor work completes", () => {
		const { events, renderer } = createRenderer();

		runWithSuspendedRenderer(renderer, () => {
			events.push("run");
		});

		expect(events).toEqual([
			"suspend",
			"clear",
			"run",
			"clear",
			"resume",
			"requestRender",
		]);
	});

	test("restores and redraws even when editor work throws", () => {
		const { events, renderer } = createRenderer();

		expect(() =>
			runWithSuspendedRenderer(renderer, () => {
				events.push("run");
				throw new Error("boom");
			}),
		).toThrow("boom");

		expect(events).toEqual([
			"suspend",
			"clear",
			"run",
			"clear",
			"resume",
			"requestRender",
		]);
	});
});
