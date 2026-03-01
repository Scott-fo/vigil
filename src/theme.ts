import { RGBA, SyntaxStyle } from "@opentui/core";
import type { Dirent } from "node:fs";
import { readdir } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

export type ThemeMode = "dark" | "light";

type HexColor = `#${string}`;
type RefName = string;
type Variant = { dark: HexColor | RefName | number; light: HexColor | RefName | number };
type ColorValue = HexColor | RefName | Variant | number | RGBA;

type ThemeJson = {
  $schema?: string;
  defs?: Record<string, HexColor | RefName | number>;
  theme: Record<string, ColorValue | number | undefined>;
};

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

function resolveTheme(themeJson: ThemeJson, mode: ThemeMode): ResolvedTheme {
  const defs = themeJson.defs ?? {};

  function resolveColor(value: ColorValue | number | undefined): RGBA {
    if (value instanceof RGBA) {
      return value;
    }

    if (typeof value === "number") {
      return ansiToRgba(value);
    }

    if (typeof value === "string") {
      if (value === "transparent" || value === "none") {
        return RGBA.fromInts(0, 0, 0, 0);
      }

      if (value.startsWith("#")) {
        return RGBA.fromHex(value);
      }

      if (defs[value] !== undefined) {
        return resolveColor(defs[value]);
      }

      if (themeJson.theme[value] !== undefined) {
        return resolveColor(themeJson.theme[value]);
      }

      throw new Error(`Color reference \"${value}\" not found.`);
    }

    if (value && typeof value === "object" && "dark" in value && "light" in value) {
      return resolveColor(mode === "dark" ? value.dark : value.light);
    }

    throw new Error(`Invalid color value: ${String(value)}`);
  }

  const resolved = {} as ThemeColors;

  for (const key of REQUIRED_THEME_KEYS) {
    const candidate = themeJson.theme[key];
    if (candidate === undefined) {
      throw new Error(`Theme is missing required color \"${key}\".`);
    }
    resolved[key] = resolveColor(candidate);
  }

  const selectedListItemText = themeJson.theme.selectedListItemText;
  resolved.selectedListItemText = selectedListItemText
    ? resolveColor(selectedListItemText)
    : resolved.background;

  const backgroundMenu = themeJson.theme.backgroundMenu;
  resolved.backgroundMenu = backgroundMenu ? resolveColor(backgroundMenu) : resolved.backgroundElement;

  return {
    ...resolved,
    _hasSelectedListItemText: selectedListItemText !== undefined,
    thinkingOpacity:
      typeof themeJson.theme.thinkingOpacity === "number" ? Math.max(0, Math.min(1, themeJson.theme.thinkingOpacity)) : 0.6,
  };
}

async function loadThemesFromDirectory(directory: string): Promise<Record<string, ThemeJson>> {
  const themes: Record<string, ThemeJson> = {};
  let entries: Dirent[];

  try {
    entries = await readdir(directory, { withFileTypes: true, encoding: "utf8" });
  } catch {
    return themes;
  }

  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith(".json")) {
      continue;
    }

    const filePath = path.join(directory, entry.name);
    try {
      const parsed = JSON.parse(await Bun.file(filePath).text()) as ThemeJson;
      if (!parsed || typeof parsed !== "object" || !parsed.theme || typeof parsed.theme !== "object") {
        continue;
      }

      const name = path.basename(entry.name, ".json");
      themes[name] = parsed;
    } catch {
      continue;
    }
  }

  return themes;
}

function getCustomThemeDirectories(): string[] {
  const directories: string[] = [];

  const xdgConfigHome = process.env.XDG_CONFIG_HOME;
  if (xdgConfigHome) {
    directories.push(path.join(xdgConfigHome, "opencode", "themes"));
  } else {
    directories.push(path.join(os.homedir(), ".config", "opencode", "themes"));
  }

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

  upward.reverse();

  const ordered = [...directories, ...upward];
  const seen = new Set<string>();
  const deduped: string[] = [];
  for (const directory of ordered) {
    if (!seen.has(directory)) {
      seen.add(directory);
      deduped.push(directory);
    }
  }

  return deduped;
}

function normalizeThemeOrder(names: string[]): string[] {
  return [...names].sort((a, b) => {
    if (a === "opencode") return -1;
    if (b === "opencode") return 1;
    return a.localeCompare(b);
  });
}

export async function loadThemeCatalog(): Promise<ThemeCatalog> {
  const themes: Record<string, ThemeJson> = {};

  Object.assign(themes, await loadThemesFromDirectory(path.join(process.cwd(), "src", "themes")));

  for (const directory of getCustomThemeDirectories()) {
    Object.assign(themes, await loadThemesFromDirectory(directory));
  }

  if (Object.keys(themes).length === 0) {
    themes.opencode = FALLBACK_THEME_JSON;
  }

  return {
    themes,
    order: normalizeThemeOrder(Object.keys(themes)),
  };
}

export function resolveThemeBundle(catalog: ThemeCatalog, requestedName: string, mode: ThemeMode): ThemeBundle {
  const fallbackName = catalog.themes.opencode ? "opencode" : (catalog.order[0] ?? "opencode");
  const selectedName = catalog.themes[requestedName] ? requestedName : fallbackName;

  const selectedThemeJson = catalog.themes[selectedName] ?? FALLBACK_THEME_JSON;

  let theme: ResolvedTheme;
  try {
    theme = resolveTheme(selectedThemeJson, mode);
  } catch {
    theme = resolveTheme(FALLBACK_THEME_JSON, mode);
  }

  return {
    name: selectedName,
    mode,
    theme,
    syntaxStyle: generateSyntax(theme),
  };
}

export function cycleThemeName(catalog: ThemeCatalog, currentName: string, direction: 1 | -1): string {
  if (catalog.order.length === 0) {
    return currentName;
  }

  const currentIndex = catalog.order.indexOf(currentName);
  const baseIndex = currentIndex === -1 ? 0 : currentIndex;
  const nextIndex = (baseIndex + direction + catalog.order.length) % catalog.order.length;
  return catalog.order[nextIndex] ?? currentName;
}

export async function readThemePreferenceFromTuiConfig(): Promise<{
  theme?: string;
  mode?: ThemeMode;
}> {
  const result: { theme?: string; mode?: ThemeMode } = {};

  try {
    const raw = await Bun.file(path.join(process.cwd(), "tui.json")).text();
    const parsed = JSON.parse(raw) as Record<string, unknown>;

    if (typeof parsed.theme === "string") {
      result.theme = parsed.theme;
    }

    const mode = parsed.theme_mode ?? parsed.mode;
    if (mode === "dark" || mode === "light") {
      result.mode = mode;
    }
  } catch {
    // Optional file.
  }

  if (process.env.REVIEWER_THEME) {
    result.theme = process.env.REVIEWER_THEME;
  }

  if (process.env.REVIEWER_THEME_MODE === "dark" || process.env.REVIEWER_THEME_MODE === "light") {
    result.mode = process.env.REVIEWER_THEME_MODE;
  }

  return result;
}

function generateSyntax(theme: ResolvedTheme): SyntaxStyle {
  return SyntaxStyle.fromTheme(getSyntaxRules(theme));
}

function getSyntaxRules(theme: ResolvedTheme) {
  return [
    { scope: ["default"], style: { foreground: theme.text } },
    { scope: ["prompt"], style: { foreground: theme.accent } },
    { scope: ["extmark.file"], style: { foreground: theme.warning, bold: true } },
    { scope: ["extmark.agent"], style: { foreground: theme.secondary, bold: true } },
    { scope: ["extmark.paste"], style: { foreground: theme.background, background: theme.warning, bold: true } },
    { scope: ["comment", "comment.documentation"], style: { foreground: theme.syntaxComment, italic: true } },
    { scope: ["string", "symbol", "character"], style: { foreground: theme.syntaxString } },
    { scope: ["number", "boolean", "float", "constant"], style: { foreground: theme.syntaxNumber } },
    { scope: ["character.special"], style: { foreground: theme.syntaxString } },
    {
      scope: ["keyword.return", "keyword.conditional", "keyword.repeat", "keyword.coroutine"],
      style: { foreground: theme.syntaxKeyword, italic: true },
    },
    { scope: ["keyword.type"], style: { foreground: theme.syntaxType, bold: true, italic: true } },
    { scope: ["keyword.function", "function.method"], style: { foreground: theme.syntaxFunction } },
    { scope: ["keyword", "keyword.directive", "keyword.modifier", "keyword.exception"], style: { foreground: theme.syntaxKeyword, italic: true } },
    { scope: ["keyword.import", "keyword.export"], style: { foreground: theme.syntaxKeyword } },
    {
      scope: ["operator", "keyword.operator", "punctuation.delimiter", "punctuation.special", "keyword.conditional.ternary"],
      style: { foreground: theme.syntaxOperator },
    },
    {
      scope: ["variable", "variable.parameter", "function.method.call", "function.call", "parameter", "property", "field"],
      style: { foreground: theme.syntaxVariable },
    },
    { scope: ["variable.member", "function", "constructor"], style: { foreground: theme.syntaxFunction } },
    { scope: ["type", "module", "class", "namespace", "type.definition"], style: { foreground: theme.syntaxType, bold: true } },
    { scope: ["punctuation", "punctuation.bracket"], style: { foreground: theme.syntaxPunctuation } },
    { scope: ["variable.builtin", "type.builtin", "function.builtin", "module.builtin", "constant.builtin", "variable.super"], style: { foreground: theme.error } },
    { scope: ["string.escape", "string.regexp", "tag.attribute", "attribute", "annotation"], style: { foreground: theme.warning } },
    { scope: ["tag"], style: { foreground: theme.error } },
    { scope: ["tag.delimiter"], style: { foreground: theme.syntaxOperator } },
    { scope: ["markup.heading", "markup.heading.1", "markup.heading.2", "markup.heading.3", "markup.heading.4", "markup.heading.5", "markup.heading.6"], style: { foreground: theme.markdownHeading, bold: true } },
    { scope: ["markup.bold", "markup.strong"], style: { foreground: theme.markdownStrong, bold: true } },
    { scope: ["markup.italic"], style: { foreground: theme.markdownEmph, italic: true } },
    { scope: ["markup.list"], style: { foreground: theme.markdownListItem } },
    { scope: ["markup.list.checked"], style: { foreground: theme.success } },
    { scope: ["markup.list.unchecked", "markup.strikethrough", "conceal"], style: { foreground: theme.textMuted } },
    { scope: ["markup.quote"], style: { foreground: theme.markdownBlockQuote, italic: true } },
    { scope: ["markup.raw", "markup.raw.block", "markdown.inline", "markup.raw.inline"], style: { foreground: theme.markdownCode } },
    { scope: ["markup.link", "markup.link.url", "string.special", "string.special.url"], style: { foreground: theme.markdownLink, underline: true } },
    { scope: ["markup.link.label", "label"], style: { foreground: theme.markdownLinkText, underline: true } },
    { scope: ["markup.underline"], style: { foreground: theme.text, underline: true } },
    { scope: ["spell", "nospell"], style: { foreground: theme.text } },
    { scope: ["diff.plus"], style: { foreground: theme.diffAdded, background: theme.diffAddedBg } },
    { scope: ["diff.minus"], style: { foreground: theme.diffRemoved, background: theme.diffRemovedBg } },
    { scope: ["diff.delta"], style: { foreground: theme.diffContext, background: theme.diffContextBg } },
    { scope: ["comment.error", "error"], style: { foreground: theme.error, bold: true, italic: true } },
    { scope: ["comment.warning", "warning"], style: { foreground: theme.warning, bold: true, italic: true } },
    { scope: ["comment.todo", "comment.note", "info"], style: { foreground: theme.info, bold: true, italic: true } },
    { scope: ["debug"], style: { foreground: theme.textMuted } },
  ];
}
