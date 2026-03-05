import { RGBA, SyntaxStyle } from "@opentui/core";
import * as BunFileSystem from "@effect/platform-bun/BunFileSystem";
import * as FileSystem from "@effect/platform/FileSystem";
import {
	readTuiConfigObject,
	resolveTuiConfigPath,
	TuiConfigReadError,
	type TuiConfigParseError,
	type TuiConfigWriteError,
	writeTuiConfigObject,
} from "@vigil/config";
import os from "node:os";
import path from "node:path";
import { Data, Effect, Option, pipe, Schema } from "effect";

export type ThemeMode = "dark" | "light";

type HexColor = `#${string}`;
type RefName = string;
type Variant = {
	dark: HexColor | RefName | number;
	light: HexColor | RefName | number;
};
type ColorValue = HexColor | RefName | Variant | number | RGBA;

const ThemeScalarValueSchema = Schema.Union(Schema.String, Schema.Number);
const ThemeVariantSchema = Schema.Struct({
	dark: ThemeScalarValueSchema,
	light: ThemeScalarValueSchema,
});
const ThemeColorValueSchema = Schema.Union(
	ThemeScalarValueSchema,
	ThemeVariantSchema,
	Schema.instanceOf(RGBA),
);
const ThemeJsonSchema = Schema.Struct({
	$schema: Schema.optional(Schema.String),
	defs: Schema.optional(
		Schema.Record({
			key: Schema.String,
			value: ThemeScalarValueSchema,
		}),
	),
	theme: Schema.Record({
		key: Schema.String,
		value: ThemeColorValueSchema,
	}),
});
type ThemeJson = Schema.Schema.Type<typeof ThemeJsonSchema>;

const JsonUnknownFromStringSchema = Schema.parseJson();

export type ThemeColors = {
	primary: RGBA;
	secondary: RGBA;
	accent: RGBA;
	error: RGBA;
	warning: RGBA;
	success: RGBA;
	info: RGBA;
	text: RGBA;
	textMuted: RGBA;
	selectedListItemText: RGBA;
	background: RGBA;
	backgroundPanel: RGBA;
	backgroundElement: RGBA;
	backgroundMenu: RGBA;
	border: RGBA;
	borderActive: RGBA;
	borderSubtle: RGBA;
	diffAdded: RGBA;
	diffRemoved: RGBA;
	diffContext: RGBA;
	diffHunkHeader: RGBA;
	diffHighlightAdded: RGBA;
	diffHighlightRemoved: RGBA;
	diffAddedBg: RGBA;
	diffRemovedBg: RGBA;
	diffContextBg: RGBA;
	diffLineNumber: RGBA;
	diffAddedLineNumberBg: RGBA;
	diffRemovedLineNumberBg: RGBA;
	markdownText: RGBA;
	markdownHeading: RGBA;
	markdownLink: RGBA;
	markdownLinkText: RGBA;
	markdownCode: RGBA;
	markdownBlockQuote: RGBA;
	markdownEmph: RGBA;
	markdownStrong: RGBA;
	markdownHorizontalRule: RGBA;
	markdownListItem: RGBA;
	markdownListEnumeration: RGBA;
	markdownImage: RGBA;
	markdownImageText: RGBA;
	markdownCodeBlock: RGBA;
	syntaxComment: RGBA;
	syntaxKeyword: RGBA;
	syntaxFunction: RGBA;
	syntaxVariable: RGBA;
	syntaxString: RGBA;
	syntaxNumber: RGBA;
	syntaxType: RGBA;
	syntaxOperator: RGBA;
	syntaxPunctuation: RGBA;
};

export type ResolvedTheme = ThemeColors & {
	_hasSelectedListItemText: boolean;
	thinkingOpacity: number;
};

export type ThemeCatalog = {
	themes: Record<string, ThemeJson>;
	order: string[];
};

export type ThemeBundle = {
	name: string;
	mode: ThemeMode;
	theme: ResolvedTheme;
	syntaxStyle: SyntaxStyle;
};

class ThemeResolutionError extends Data.TaggedError("ThemeResolutionError")<{
	readonly message: string;
}> {}

class ThemeFileReadError extends Data.TaggedError("ThemeFileReadError")<{
	readonly filePath: string;
	readonly message: string;
}> {}

class ThemeFileParseError extends Data.TaggedError("ThemeFileParseError")<{
	readonly filePath: string;
	readonly message: string;
}> {}

class ThemeFileInvalidError extends Data.TaggedError("ThemeFileInvalidError")<{
	readonly filePath: string;
}> {}

type ThemeFileError =
	| ThemeFileReadError
	| ThemeFileParseError
	| ThemeFileInvalidError;

export type ThemePreferencePersistError =
	| TuiConfigReadError
	| TuiConfigParseError
	| TuiConfigWriteError;

const REQUIRED_THEME_KEYS = [
	"primary",
	"secondary",
	"accent",
	"error",
	"warning",
	"success",
	"info",
	"text",
	"textMuted",
	"background",
	"backgroundPanel",
	"backgroundElement",
	"border",
	"borderActive",
	"borderSubtle",
	"diffAdded",
	"diffRemoved",
	"diffContext",
	"diffHunkHeader",
	"diffHighlightAdded",
	"diffHighlightRemoved",
	"diffAddedBg",
	"diffRemovedBg",
	"diffContextBg",
	"diffLineNumber",
	"diffAddedLineNumberBg",
	"diffRemovedLineNumberBg",
	"markdownText",
	"markdownHeading",
	"markdownLink",
	"markdownLinkText",
	"markdownCode",
	"markdownBlockQuote",
	"markdownEmph",
	"markdownStrong",
	"markdownHorizontalRule",
	"markdownListItem",
	"markdownListEnumeration",
	"markdownImage",
	"markdownImageText",
	"markdownCodeBlock",
	"syntaxComment",
	"syntaxKeyword",
	"syntaxFunction",
	"syntaxVariable",
	"syntaxString",
	"syntaxNumber",
	"syntaxType",
	"syntaxOperator",
	"syntaxPunctuation",
] as const;

const FALLBACK_THEME_JSON: ThemeJson = {
	theme: {
		primary: "#fab283",
		secondary: "#5c9cf5",
		accent: "#9d7cd8",
		error: "#e06c75",
		warning: "#f5a742",
		success: "#7fd88f",
		info: "#56b6c2",
		text: "#eeeeee",
		textMuted: "#808080",
		background: "#0a0a0a",
		backgroundPanel: "#141414",
		backgroundElement: "#1e1e1e",
		border: "#484848",
		borderActive: "#606060",
		borderSubtle: "#3c3c3c",
		diffAdded: "#4fd6be",
		diffRemoved: "#c53b53",
		diffContext: "#828bb8",
		diffHunkHeader: "#828bb8",
		diffHighlightAdded: "#b8db87",
		diffHighlightRemoved: "#e26a75",
		diffAddedBg: "#20303b",
		diffRemovedBg: "#37222c",
		diffContextBg: "#141414",
		diffLineNumber: "#1e1e1e",
		diffAddedLineNumberBg: "#1b2b34",
		diffRemovedLineNumberBg: "#2d1f26",
		markdownText: "#eeeeee",
		markdownHeading: "#9d7cd8",
		markdownLink: "#fab283",
		markdownLinkText: "#56b6c2",
		markdownCode: "#7fd88f",
		markdownBlockQuote: "#e5c07b",
		markdownEmph: "#e5c07b",
		markdownStrong: "#f5a742",
		markdownHorizontalRule: "#808080",
		markdownListItem: "#fab283",
		markdownListEnumeration: "#56b6c2",
		markdownImage: "#fab283",
		markdownImageText: "#56b6c2",
		markdownCodeBlock: "#eeeeee",
		syntaxComment: "#808080",
		syntaxKeyword: "#9d7cd8",
		syntaxFunction: "#fab283",
		syntaxVariable: "#e06c75",
		syntaxString: "#7fd88f",
		syntaxNumber: "#f5a742",
		syntaxType: "#e5c07b",
		syntaxOperator: "#56b6c2",
		syntaxPunctuation: "#eeeeee",
	},
};

function ansiToRgba(code: number): RGBA {
	if (code < 16) {
		const ansiColors = [
			"#000000",
			"#800000",
			"#008000",
			"#808000",
			"#000080",
			"#800080",
			"#008080",
			"#c0c0c0",
			"#808080",
			"#ff0000",
			"#00ff00",
			"#ffff00",
			"#0000ff",
			"#ff00ff",
			"#00ffff",
			"#ffffff",
		];
		return RGBA.fromHex(ansiColors[code] ?? "#000000");
	}

	if (code < 232) {
		const index = code - 16;
		const b = index % 6;
		const g = Math.floor(index / 6) % 6;
		const r = Math.floor(index / 36);
		const val = (x: number) => (x === 0 ? 0 : x * 40 + 55);
		return RGBA.fromInts(val(r), val(g), val(b));
	}

	if (code < 256) {
		const gray = (code - 232) * 10 + 8;
		return RGBA.fromInts(gray, gray, gray);
	}

	return RGBA.fromInts(0, 0, 0);
}

function resolveTheme(
	themeJson: ThemeJson,
	mode: ThemeMode,
): Effect.Effect<ResolvedTheme, ThemeResolutionError> {
	const defs = themeJson.defs ?? {};

	function resolveColor(
		value: ColorValue | number | undefined,
	): Effect.Effect<RGBA, ThemeResolutionError> {
		if (value instanceof RGBA) {
			return Effect.succeed(value);
		}

		if (typeof value === "number") {
			return Effect.succeed(ansiToRgba(value));
		}

		if (typeof value === "string") {
			if (value === "transparent" || value === "none") {
				return Effect.succeed(RGBA.fromInts(0, 0, 0, 0));
			}

			if (value.startsWith("#")) {
				return Effect.try({
					try: () => RGBA.fromHex(value),
					catch: () =>
						new ThemeResolutionError({
							message: `Invalid hex color "${value}".`,
						}),
				});
			}

			if (defs[value] !== undefined) {
				return resolveColor(defs[value]);
			}

			if (themeJson.theme[value] !== undefined) {
				return resolveColor(themeJson.theme[value]);
			}

			return Effect.fail(
				new ThemeResolutionError({
					message: `Color reference "${value}" not found.`,
				}),
			);
		}

		if (
			value &&
			typeof value === "object" &&
			"dark" in value &&
			"light" in value
		) {
			return resolveColor(mode === "dark" ? value.dark : value.light);
		}

		return Effect.fail(
			new ThemeResolutionError({
				message: `Invalid color value: ${String(value)}`,
			}),
		);
	}

	return Effect.gen(function* () {
		const resolved = {} as ThemeColors;

		for (const key of REQUIRED_THEME_KEYS) {
			const candidate = themeJson.theme[key];
			if (candidate === undefined) {
				yield* Effect.fail(
					new ThemeResolutionError({
						message: `Theme is missing required color "${key}".`,
					}),
				);
			}
			resolved[key] = yield* resolveColor(candidate);
		}

		const selectedListItemText = themeJson.theme.selectedListItemText;
		resolved.selectedListItemText = selectedListItemText
			? yield* resolveColor(selectedListItemText)
			: resolved.background;

		const backgroundMenu = themeJson.theme.backgroundMenu;
		resolved.backgroundMenu = backgroundMenu
			? yield* resolveColor(backgroundMenu)
			: resolved.backgroundElement;

		return {
			...resolved,
			_hasSelectedListItemText: selectedListItemText !== undefined,
			thinkingOpacity:
				typeof themeJson.theme.thinkingOpacity === "number"
					? Math.max(0, Math.min(1, themeJson.theme.thinkingOpacity))
					: 0.6,
		};
	});
}

function loadThemeFile(
	filePath: string,
): Effect.Effect<ThemeJson, ThemeFileError, FileSystem.FileSystem> {
	return Effect.gen(function* () {
		const fs = yield* FileSystem.FileSystem;
		const raw = yield* pipe(
			fs.readFileString(filePath),
			Effect.catchTag("SystemError", (cause) =>
				Effect.fail(
					new ThemeFileReadError({
						filePath,
						message: cause.message,
					}),
				),
			),
			Effect.catchTag("BadArgument", (cause) =>
				Effect.fail(
					new ThemeFileReadError({
						filePath,
						message: cause.message,
					}),
				),
			),
		);

		const parsed = yield* pipe(
			Schema.decodeUnknown(JsonUnknownFromStringSchema)(raw),
			Effect.mapError(
				() =>
					new ThemeFileParseError({
						filePath,
						message: "Invalid JSON in theme file.",
					}),
			),
		);

		return yield* pipe(
			Schema.decodeUnknown(ThemeJsonSchema)(parsed),
			Effect.mapError(() => new ThemeFileInvalidError({ filePath })),
		);
	});
}

async function loadThemesFromDirectory(
	directory: string,
): Promise<Record<string, ThemeJson>> {
	return Effect.runPromise(
		pipe(
			Effect.gen(function* () {
				const fs = yield* FileSystem.FileSystem;
				const entries = yield* pipe(
					fs.readDirectory(directory),
					Effect.orElseSucceed(() => [] as string[]),
				);

				const themes: Record<string, ThemeJson> = {};
				for (const entry of entries) {
					if (!entry.endsWith(".json")) {
						continue;
					}

					const filePath = path.join(directory, entry);
					const parsedTheme = yield* pipe(loadThemeFile(filePath), Effect.option);
					pipe(
						parsedTheme,
						Option.match({
							onNone: () => undefined,
							onSome: (theme) => {
								const themeName = path.basename(entry, ".json");
								themes[themeName] = theme;
							},
						}),
					);
				}

				return themes;
			}),
			Effect.provide(BunFileSystem.layer),
		),
	);
}

function getCustomThemeDirectories(): string[] {
	const globalThemesDirectory = process.env.XDG_CONFIG_HOME
		? path.join(process.env.XDG_CONFIG_HOME, "opencode", "themes")
		: path.join(os.homedir(), ".config", "opencode", "themes");
	const upward: string[] = [];
	let current = process.cwd();

	for (;;) {
		upward.push(path.join(current, ".opencode", "themes"));
		const parent = path.dirname(current);
		if (parent === current) {
			break;
		}
		current = parent;
	}

	return Array.from(new Set([globalThemesDirectory, ...upward.reverse()]));
}

function normalizeThemeOrder(names: string[]): string[] {
	return [...names].sort((a, b) => {
		if (a === "opencode") return -1;
		if (b === "opencode") return 1;
		return a.localeCompare(b);
	});
}

function getBundledThemeDirectories(): string[] {
	const installedThemesDirectory = path.join(path.dirname(process.execPath), "themes");
	const sourceThemesDirectory = path.join(import.meta.dir, "..", "themes");

	return Array.from(
		new Set([installedThemesDirectory, sourceThemesDirectory]),
	);
}

export async function loadThemeCatalog(): Promise<ThemeCatalog> {
	const directories = [
		...getBundledThemeDirectories(),
		...getCustomThemeDirectories(),
	];
	const layers = await Promise.all(directories.map(loadThemesFromDirectory));
	const themes = Object.assign({}, ...layers) as Record<string, ThemeJson>;

	const resolvedThemes =
		Object.keys(themes).length === 0
			? { opencode: FALLBACK_THEME_JSON }
			: themes;

	return {
		themes: resolvedThemes,
		order: normalizeThemeOrder(Object.keys(resolvedThemes)),
	};
}

export function resolveThemeBundle(
	catalog: ThemeCatalog,
	requestedName: string,
	mode: ThemeMode,
): ThemeBundle {
	const fallbackName = catalog.themes.opencode
		? "opencode"
		: (catalog.order[0] ?? "opencode");
	const selectedName = catalog.themes[requestedName]
		? requestedName
		: fallbackName;

	const selectedThemeJson = catalog.themes[selectedName] ?? FALLBACK_THEME_JSON;

	const theme = Effect.runSync(
		pipe(
			resolveTheme(selectedThemeJson, mode),
			Effect.catchAll(() => resolveTheme(FALLBACK_THEME_JSON, mode)),
			Effect.orDie,
		),
	);

	return {
		name: selectedName,
		mode,
		theme,
		syntaxStyle: generateSyntax(theme),
	};
}

export function cycleThemeName(
	catalog: ThemeCatalog,
	currentName: string,
	direction: 1 | -1,
): string {
	if (catalog.order.length === 0) {
		return currentName;
	}

	const currentIndex = catalog.order.indexOf(currentName);
	const baseIndex = currentIndex === -1 ? 0 : currentIndex;
	const nextIndex =
		(baseIndex + direction + catalog.order.length) % catalog.order.length;
	return catalog.order[nextIndex] ?? currentName;
}

type ThemePreference = {
	theme?: string;
	mode?: ThemeMode;
};

const LEGACY_TUI_CONFIG_FILE = "tui.json";

function resolveLegacyConfigFilePath(): string {
	return path.join(process.cwd(), LEGACY_TUI_CONFIG_FILE);
}

function parseThemePreference(raw: Record<string, unknown>): ThemePreference {
	const parsed: ThemePreference = {};
	if (typeof raw.theme === "string") {
		parsed.theme = raw.theme;
	}

	const mode = raw.theme_mode ?? raw.mode;
	if (mode === "dark" || mode === "light") {
		parsed.mode = mode;
	}

	return parsed;
}

function applyEnvThemePreference(preference: ThemePreference): ThemePreference {
	const envTheme = process.env.VIGIL_THEME ?? process.env.REVIEWER_THEME;
	const envMode =
		process.env.VIGIL_THEME_MODE ?? process.env.REVIEWER_THEME_MODE;
	return {
		...(envTheme
			? { theme: envTheme }
			: preference.theme !== undefined
				? { theme: preference.theme }
				: {}),
		...(envMode === "dark" || envMode === "light"
			? { mode: envMode }
			: preference.mode !== undefined
				? { mode: preference.mode }
				: {}),
	};
}

export async function readThemePreferenceFromTuiConfig(): Promise<{
	theme?: string;
	mode?: ThemeMode;
}> {
	const filePreference = await Effect.runPromise(
		pipe(
			Effect.gen(function* () {
				const fs = yield* FileSystem.FileSystem;
				const primaryPath = resolveTuiConfigPath();
				const legacyPath = resolveLegacyConfigFilePath();
				const primaryExists = yield* pipe(
					fs.exists(primaryPath),
					Effect.catchTag("SystemError", (cause) =>
						Effect.fail(
							new TuiConfigReadError({
								filePath: primaryPath,
								message: cause.message,
								cause,
							}),
						),
					),
					Effect.catchTag("BadArgument", (cause) =>
						Effect.fail(
							new TuiConfigReadError({
								filePath: primaryPath,
								message: cause.message,
								cause,
							}),
						),
					),
				);
				const configPath = primaryExists ? primaryPath : legacyPath;
				const config = yield* readTuiConfigObject(configPath);
				return parseThemePreference(config);
			}),
			Effect.orElseSucceed(() => ({}) as ThemePreference),
			Effect.provide(BunFileSystem.layer),
		),
	);

	return applyEnvThemePreference(filePreference);
}

export function persistThemePreferenceToTuiConfig(preference: {
	readonly theme: string;
	readonly mode: ThemeMode;
}): Effect.Effect<void, ThemePreferencePersistError> {
	const filePath = resolveTuiConfigPath();

	return pipe(
		Effect.gen(function* () {
			const config = yield* readTuiConfigObject(filePath);
			const nextConfig: Record<string, unknown> = {
				...config,
				theme: preference.theme,
				theme_mode: preference.mode,
			};
			yield* writeTuiConfigObject(nextConfig, filePath);
		}),
		Effect.provide(BunFileSystem.layer),
	);
}

function generateSyntax(theme: ResolvedTheme): SyntaxStyle {
	return SyntaxStyle.fromTheme(getSyntaxRules(theme));
}

function getSyntaxRules(theme: ResolvedTheme) {
	return [
		{ scope: ["default"], style: { foreground: theme.text } },
		{ scope: ["prompt"], style: { foreground: theme.accent } },
		{
			scope: ["extmark.file"],
			style: { foreground: theme.warning, bold: true },
		},
		{
			scope: ["extmark.agent"],
			style: { foreground: theme.secondary, bold: true },
		},
		{
			scope: ["extmark.paste"],
			style: {
				foreground: theme.background,
				background: theme.warning,
				bold: true,
			},
		},
		{
			scope: ["comment", "comment.documentation"],
			style: { foreground: theme.syntaxComment, italic: true },
		},
		{
			scope: ["string", "symbol", "character"],
			style: { foreground: theme.syntaxString },
		},
		{
			scope: ["number", "boolean", "float", "constant"],
			style: { foreground: theme.syntaxNumber },
		},
		{ scope: ["character.special"], style: { foreground: theme.syntaxString } },
		{
			scope: [
				"keyword.return",
				"keyword.conditional",
				"keyword.repeat",
				"keyword.coroutine",
			],
			style: { foreground: theme.syntaxKeyword, italic: true },
		},
		{
			scope: ["keyword.type"],
			style: { foreground: theme.syntaxType, bold: true, italic: true },
		},
		{
			scope: ["keyword.function", "function.method"],
			style: { foreground: theme.syntaxFunction },
		},
		{
			scope: [
				"keyword",
				"keyword.directive",
				"keyword.modifier",
				"keyword.exception",
			],
			style: { foreground: theme.syntaxKeyword, italic: true },
		},
		{
			scope: ["keyword.import", "keyword.export"],
			style: { foreground: theme.syntaxKeyword },
		},
		{
			scope: [
				"operator",
				"keyword.operator",
				"punctuation.delimiter",
				"punctuation.special",
				"keyword.conditional.ternary",
			],
			style: { foreground: theme.syntaxOperator },
		},
		{
			scope: [
				"variable",
				"variable.parameter",
				"function.method.call",
				"function.call",
				"parameter",
				"property",
				"field",
			],
			style: { foreground: theme.syntaxVariable },
		},
		{
			scope: ["variable.member", "function", "constructor"],
			style: { foreground: theme.syntaxFunction },
		},
		{
			scope: ["type", "module", "class", "namespace", "type.definition"],
			style: { foreground: theme.syntaxType, bold: true },
		},
		{
			scope: ["punctuation", "punctuation.bracket"],
			style: { foreground: theme.syntaxPunctuation },
		},
		{
			scope: [
				"variable.builtin",
				"type.builtin",
				"function.builtin",
				"module.builtin",
				"constant.builtin",
				"variable.super",
			],
			style: { foreground: theme.error },
		},
		{
			scope: [
				"string.escape",
				"string.regexp",
				"tag.attribute",
				"attribute",
				"annotation",
			],
			style: { foreground: theme.warning },
		},
		{ scope: ["tag"], style: { foreground: theme.error } },
		{ scope: ["tag.delimiter"], style: { foreground: theme.syntaxOperator } },
		{
			scope: [
				"markup.heading",
				"markup.heading.1",
				"markup.heading.2",
				"markup.heading.3",
				"markup.heading.4",
				"markup.heading.5",
				"markup.heading.6",
			],
			style: { foreground: theme.markdownHeading, bold: true },
		},
		{
			scope: ["markup.bold", "markup.strong"],
			style: { foreground: theme.markdownStrong, bold: true },
		},
		{
			scope: ["markup.italic"],
			style: { foreground: theme.markdownEmph, italic: true },
		},
		{ scope: ["markup.list"], style: { foreground: theme.markdownListItem } },
		{ scope: ["markup.list.checked"], style: { foreground: theme.success } },
		{
			scope: ["markup.list.unchecked", "markup.strikethrough", "conceal"],
			style: { foreground: theme.textMuted },
		},
		{
			scope: ["markup.quote"],
			style: { foreground: theme.markdownBlockQuote, italic: true },
		},
		{
			scope: [
				"markup.raw",
				"markup.raw.block",
				"markdown.inline",
				"markup.raw.inline",
			],
			style: { foreground: theme.markdownCode },
		},
		{
			scope: [
				"markup.link",
				"markup.link.url",
				"string.special",
				"string.special.url",
			],
			style: { foreground: theme.markdownLink, underline: true },
		},
		{
			scope: ["markup.link.label", "label"],
			style: { foreground: theme.markdownLinkText, underline: true },
		},
		{
			scope: ["markup.underline"],
			style: { foreground: theme.text, underline: true },
		},
		{ scope: ["spell", "nospell"], style: { foreground: theme.text } },
		{
			scope: ["diff.plus"],
			style: { foreground: theme.diffAdded, background: theme.diffAddedBg },
		},
		{
			scope: ["diff.minus"],
			style: { foreground: theme.diffRemoved, background: theme.diffRemovedBg },
		},
		{
			scope: ["diff.delta"],
			style: { foreground: theme.diffContext, background: theme.diffContextBg },
		},
		{
			scope: ["comment.error", "error"],
			style: { foreground: theme.error, bold: true, italic: true },
		},
		{
			scope: ["comment.warning", "warning"],
			style: { foreground: theme.warning, bold: true, italic: true },
		},
		{
			scope: ["comment.todo", "comment.note", "info"],
			style: { foreground: theme.info, bold: true, italic: true },
		},
		{ scope: ["debug"], style: { foreground: theme.textMuted } },
	];
}
