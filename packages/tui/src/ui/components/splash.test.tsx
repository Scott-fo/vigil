import { describe, expect, test } from "bun:test";
import { RGBA } from "@opentui/core";
import { testRender } from "@opentui/react/test-utils";
import { Option } from "effect";
import { act } from "react";
import type { ResolvedTheme } from "#theme/theme";
import { Splash } from "#ui/components/splash";

const theme = {
	text: RGBA.fromInts(255, 255, 255),
	textMuted: RGBA.fromInts(180, 180, 180),
} as unknown as ResolvedTheme;

describe("Splash", () => {
	test("renders clean working tree message by default", async () => {
		const setup = await testRender(<Splash theme={theme} error={Option.none()} />, {
			width: 120,
			height: 30,
		});
		try {
			await act(async () => {
				await setup.renderOnce();
			});
			expect(setup.captureCharFrame()).toContain("No changed files in working tree");
		} finally {
			await act(async () => {
				setup.renderer.destroy();
			});
		}
	});

	test("renders git-init guidance for non-repo error", async () => {
		const setup = await testRender(
			<Splash theme={theme} error={Option.some("Not a git repository")} />,
			{
				width: 120,
				height: 30,
			},
		);
		try {
			await act(async () => {
				await setup.renderOnce();
			});
			expect(setup.captureCharFrame()).toContain(
				"Not a git repo, init to use reviewer.",
			);
			expect(setup.captureCharFrame()).toContain("Press i to git init.");
		} finally {
			await act(async () => {
				setup.renderer.destroy();
			});
		}
	});

	test("renders non-git errors as-is", async () => {
		const setup = await testRender(
			<Splash theme={theme} error={Option.some("Permission denied")} />,
			{
				width: 120,
				height: 30,
			},
		);
		try {
			await act(async () => {
				await setup.renderOnce();
			});
			expect(setup.captureCharFrame()).toContain("Permission denied");
		} finally {
			await act(async () => {
				setup.renderer.destroy();
			});
		}
	});
});
