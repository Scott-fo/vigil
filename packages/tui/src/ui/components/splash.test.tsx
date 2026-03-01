import { describe, expect, test } from "bun:test";
import { RGBA } from "@opentui/core";
import { testRender } from "@opentui/react/test-utils";
import { act } from "react";
import type { ResolvedTheme } from "#theme/theme";
import { Splash } from "#ui/components/splash";

const theme = {
	text: RGBA.fromInts(255, 255, 255),
	textMuted: RGBA.fromInts(180, 180, 180),
} as unknown as ResolvedTheme;

describe("Splash", () => {
	test("renders reviewer onboarding message", async () => {
		const setup = await testRender(<Splash theme={theme} />, {
			width: 120,
			height: 30,
		});
		try {
			await act(async () => {
				await setup.renderOnce();
			});
			expect(setup.captureCharFrame()).toContain(
				"Initialise git repo to use Reviewer",
			);
		} finally {
			await act(async () => {
				setup.renderer.destroy();
			});
		}
	});
});
