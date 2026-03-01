import {
  addDefaultParsers,
  getTreeSitterClient,
  pathToFiletype,
  type FiletypeParserOptions,
  type InjectionMapping,
  type TreeSitterClient,
} from "@opentui/core";
import parserConfig from "#config/parsers";

type RawParserConfig = {
  parsers?: unknown;
};

type RawParser = {
  filetype?: unknown;
  wasm?: unknown;
  queries?: {
    highlights?: unknown;
    injections?: unknown;
  };
  injectionMapping?: unknown;
};

let parserOptionsCache: FiletypeParserOptions[] | null = null;
let treeSitterInitPromise: Promise<TreeSitterClient> | null = null;

function toStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.filter((item): item is string => typeof item === "string" && item.length > 0);
}

function asInjectionMapping(value: unknown): InjectionMapping | undefined {
  if (!value || typeof value !== "object") {
    return undefined;
  }
  return value as InjectionMapping;
}

function normalizeParser(raw: RawParser): FiletypeParserOptions | null {
  if (typeof raw.filetype !== "string" || raw.filetype.length === 0) {
    return null;
  }

  if (typeof raw.wasm !== "string" || raw.wasm.length === 0) {
    return null;
  }

  const highlights = toStringArray(raw.queries?.highlights);
  if (highlights.length === 0) {
    return null;
  }

  const injections = toStringArray(raw.queries?.injections);

  return {
    filetype: raw.filetype,
    wasm: raw.wasm,
    queries: {
      highlights,
      ...(injections.length > 0 ? { injections } : {}),
    },
    ...(asInjectionMapping(raw.injectionMapping) ? { injectionMapping: asInjectionMapping(raw.injectionMapping) } : {}),
  };
}

function getParserOptions(): FiletypeParserOptions[] {
  if (parserOptionsCache) {
    return parserOptionsCache;
  }

  const config = parserConfig as RawParserConfig;
  const list = Array.isArray(config.parsers) ? config.parsers : [];

  parserOptionsCache = list
    .filter((item): item is RawParser => !!item && typeof item === "object")
    .map((item) => normalizeParser(item))
    .filter((item): item is FiletypeParserOptions => item !== null);

  return parserOptionsCache;
}

export async function initializeTreeSitterClient(): Promise<TreeSitterClient> {
  if (treeSitterInitPromise) {
    return treeSitterInitPromise;
  }

  treeSitterInitPromise = (async () => {
    addDefaultParsers(getParserOptions());
    const client = getTreeSitterClient();
    await client.initialize();
    return client;
  })();

  try {
    return await treeSitterInitPromise;
  } catch (error) {
    treeSitterInitPromise = null;
    throw error;
  }
}

export function resolveDiffFiletype(filePath: string): string | undefined {
  const fileName = filePath.toLowerCase().split("/").pop() ?? "";
  if (fileName === "dockerfile") {
    return "dockerfile";
  }

  const openTuiFiletype = pathToFiletype(filePath);
  if (!openTuiFiletype) {
    const ext = fileName.includes(".") ? fileName.split(".").pop() : "";
    if (ext === "diff" || ext === "patch") {
      return "diff";
    }
    return undefined;
  }

  if (openTuiFiletype === "typescriptreact" || openTuiFiletype === "javascriptreact" || openTuiFiletype === "javascript") {
    return "typescript";
  }

  if (openTuiFiletype === "shell") {
    return "bash";
  }

  return openTuiFiletype;
}
