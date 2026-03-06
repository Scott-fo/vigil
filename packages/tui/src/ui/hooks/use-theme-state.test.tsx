import { describe, expect, test } from "bun:test";
import { testRender } from "@opentui/react/test-utils";
import { useEffect, useState } from "react";
import { act } from "react";
import {
	getFallbackThemeCatalog,
	type ThemeCatalog,
	type ThemeMode,
} from "#theme/theme.ts";
import { useThemeState } from "#ui/hooks/use-theme-state.ts";

const fallbackThemeCatalog = getFallbackThemeCatalog();
const fallbackTheme =
	fallbackThemeCatalog.themes.opencode ??
	fallbackThemeCatalog.themes[fallbackThemeCatalog.order[0] ?? "opencode"]!;
const progressiveThemeCatalog: ThemeCatalog = {
	themes: {
		...fallbackThemeCatalog.themes,
		gruvbox: fallbackTheme,
	},
	order: ["opencode", "gruvbox"],
};

function ThemeStateProbe(props: {
	readonly themeCatalog: ThemeCatalog;
	readonly themeMode: ThemeMode;
	readonly themeName: string;
}) {
	const { themeBundle } = useThemeState({
		themeCatalog: props.themeCatalog,
		themeMode: props.themeMode,
		themeName: props.themeName,
		themeModal: { isOpen: false },
	});

	return (
		<box>
			<text>{themeBundle.name}</text>
		</box>
	);
}

function ProgressiveThemeProbe() {
	const [state, setState] = useState({
		themeCatalog: fallbackThemeCatalog,
		themeMode: "dark" as ThemeMode,
		themeName: "opencode",
	});

	useEffect(() => {
		setState({
			themeCatalog: progressiveThemeCatalog,
			themeMode: "dark",
			themeName: "gruvbox",
		});
	}, []);

	return <ThemeStateProbe {...state} />;
}

describe("useThemeState", () => {
	test("adopts progressive boot theme updates before any local override", async () => {
		const setup = await testRender(<ProgressiveThemeProbe />, {
			width: 60,
			height: 10,
		});

		try {
			await act(async () => {
				await setup.renderOnce();
				await Promise.resolve();
				await setup.renderOnce();
			});

			expect(setup.captureCharFrame()).toContain("gruvbox");
		} finally {
			await act(async () => {
				setup.renderer.destroy();
			});
		}
	});
});
