import { createCliRenderer } from "@opentui/core";
import { createRoot, useKeyboard, useRenderer } from "@opentui/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  cycleThemeName,
  loadThemeCatalog,
  readThemePreferenceFromTuiConfig,
  resolveThemeBundle,
  type ResolvedTheme,
  type ThemeCatalog,
  type ThemeMode,
} from "./theme";
import { initializeTreeSitterClient, resolveDiffFiletype } from "./tree-sitter";

interface StatusEntry {
  status: string;
  path: string;
  originalPath?: string;
}

interface FileEntry {
  status: string;
  path: string;
  label: string;
  diff: string;
  filetype?: string;
  note?: string;
}

interface GitCommandResult {
  ok: boolean;
  stdout: string;
  stderr: string;
}

interface AppProps {
  themeCatalog: ThemeCatalog;
  initialThemeName: string;
  initialThemeMode: ThemeMode;
}

const TEXT_DECODER = new TextDecoder();

function decodeOutput(output?: Uint8Array | null): string {
  if (!output) {
    return "";
  }
  return TEXT_DECODER.decode(output);
}

function runGit(args: string[]): GitCommandResult {
  const result = Bun.spawnSync({
    cmd: ["git", ...args],
    stdout: "pipe",
    stderr: "pipe",
  });

  return {
    ok: result.exitCode === 0,
    stdout: decodeOutput(result.stdout),
    stderr: decodeOutput(result.stderr),
  };
}

function parseStatusEntries(raw: string): StatusEntry[] {
  const entries: StatusEntry[] = [];
  const fields = raw.split("\0");
  let index = 0;

  while (index < fields.length) {
    const field = fields[index];
    index += 1;

    if (!field) {
      continue;
    }

    if (field.length < 4) {
      continue;
    }

    const x = field[0] ?? " ";
    const y = field[1] ?? " ";
    const status = `${x}${y}`;
    const firstPath = field.slice(3);

    if (!firstPath) {
      continue;
    }

    if (x === "R" || x === "C") {
      const renamedTo = fields[index];
      index += 1;
      entries.push({
        status,
        path: renamedTo || firstPath,
        originalPath: firstPath,
      });
      continue;
    }

    entries.push({ status, path: firstPath });
  }

  return entries;
}

function inferFiletype(inputPath: string): string | undefined {
  return resolveDiffFiletype(inputPath);
}

function getStatusColor(status: string, theme: ResolvedTheme) {
  if (status === "??" || status.includes("A")) {
    return theme.diffHighlightAdded;
  }
  if (status.includes("U") || status.includes("D")) {
    return theme.diffHighlightRemoved;
  }
  if (status.includes("R") || status.includes("C")) {
    return theme.accent;
  }
  if (status.includes("M")) {
    return theme.warning;
  }
  return theme.textMuted;
}

function createUntrackedFileDiff(inputPath: string, content: string): string {
  const normalized = content.replace(/\r\n/g, "\n");
  if (normalized.length === 0) {
    return "";
  }

  const hasTrailingNewline = normalized.endsWith("\n");
  const lines = normalized.split("\n");

  if (hasTrailingNewline) {
    lines.pop();
  }

  const lineCount = lines.length;
  const hunkHeader = `@@ -0,0 +1,${lineCount} @@`;
  let body = lines.map((line) => `+${line}`).join("\n");

  if (lineCount > 0 && hasTrailingNewline) {
    body += "\n";
  }

  return [
    `diff --git a/${inputPath} b/${inputPath}`,
    "new file mode 100644",
    "index 0000000..1111111",
    "--- /dev/null",
    `+++ b/${inputPath}`,
    hunkHeader,
    body,
    "",
  ].join("\n");
}

async function loadFilesWithDiffs(): Promise<{ files: FileEntry[]; error?: string }> {
  const statusResult = runGit(["status", "--porcelain=v1", "-z", "--untracked-files=all"]);
  if (!statusResult.ok) {
    return {
      files: [],
      error: statusResult.stderr.trim() || "Unable to run git status.",
    };
  }

  const statusEntries = parseStatusEntries(statusResult.stdout).filter((entry) => entry.status !== "!!");
  const files: FileEntry[] = [];

  for (const entry of statusEntries) {
    const label = entry.originalPath ? `${entry.originalPath} -> ${entry.path}` : entry.path;
    let diff = "";
    let note: string | undefined;

    if (entry.status === "??") {
      try {
        const bytes = await Bun.file(entry.path).bytes();
        const hasNullByte = bytes.includes(0);

        if (hasNullByte) {
          note = "Binary or non-text file; no preview available.";
        } else {
          const content = TEXT_DECODER.decode(bytes);
          diff = createUntrackedFileDiff(entry.path, content);
          if (!diff.trim()) {
            note = "Untracked empty file; no textual hunk to preview.";
          }
        }
      } catch {
        note = "Unable to read untracked file content.";
      }
    } else {
      const diffResult = runGit(["diff", "--no-color", "--find-renames", "HEAD", "--", entry.path]);
      if (diffResult.ok) {
        diff = diffResult.stdout;
      } else {
        note = diffResult.stderr.trim() || "Unable to load diff for this file.";
      }
    }

    if (!diff.trim() && !note) {
      note = "No textual diff available.";
    }

    files.push({
      status: entry.status,
      path: entry.path,
      label,
      diff,
      filetype: inferFiletype(entry.path),
      note,
    });
  }

  return { files };
}

function App(props: AppProps) {
  const renderer = useRenderer();
  const [themeName, setThemeName] = useState(props.initialThemeName);
  const [themeMode, setThemeMode] = useState<ThemeMode>(props.initialThemeMode);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const isRefreshingRef = useRef(false);

  const themeBundle = useMemo(
    () => resolveThemeBundle(props.themeCatalog, themeName, themeMode),
    [props.themeCatalog, themeName, themeMode],
  );
  const theme = themeBundle.theme;

  const refreshFiles = useCallback(async (showLoading: boolean) => {
    if (isRefreshingRef.current) {
      return;
    }

    isRefreshingRef.current = true;
    if (showLoading) {
      setLoading(true);
    }
    try {
      const result = await loadFilesWithDiffs();

      setFiles(result.files);
      setError(result.error ?? null);

      setSelectedPath((current) => {
        if (result.files.length === 0) {
          return null;
        }
        if (current && result.files.some((file) => file.path === current)) {
          return current;
        }
        return result.files[0]?.path ?? null;
      });
    } finally {
      if (showLoading) {
        setLoading(false);
      }
      isRefreshingRef.current = false;
    }
  }, []);

  useEffect(() => {
    void refreshFiles(true);
  }, [refreshFiles]);

  useEffect(() => {
    const interval = setInterval(() => {
      void refreshFiles(false);
    }, 2000);

    return () => clearInterval(interval);
  }, [refreshFiles]);

  const selectedFile = useMemo(() => {
    if (files.length === 0) {
      return null;
    }
    if (selectedPath) {
      const match = files.find((file) => file.path === selectedPath);
      if (match) {
        return match;
      }
    }
    return files[0] ?? null;
  }, [files, selectedPath]);

  const selectedIndex = selectedFile ? files.findIndex((file) => file.path === selectedFile.path) : -1;

  useKeyboard((key) => {
    if ((key.ctrl && key.name === "c") || key.name === "escape" || key.name === "q") {
      renderer.destroy();
      return;
    }

    if (!key.ctrl && !key.meta && key.name === "t") {
      setThemeName((current) => cycleThemeName(props.themeCatalog, current, key.shift ? -1 : 1));
      return;
    }

    if (files.length === 0 || selectedIndex === -1) {
      return;
    }

    if (key.name === "down" || key.name === "j") {
      const nextIndex = Math.min(selectedIndex + 1, files.length - 1);
      setSelectedPath(files[nextIndex]?.path ?? null);
      return;
    }

    if (key.name === "up" || key.name === "k") {
      const nextIndex = Math.max(selectedIndex - 1, 0);
      setSelectedPath(files[nextIndex]?.path ?? null);
    }
  });

  return (
    <box flexDirection="column" flexGrow={1} padding={1} backgroundColor={theme.background}>
      <box flexDirection="row" flexGrow={1}>
        <box
          width={44}
          border
          borderStyle="rounded"
          borderColor={theme.border}
          marginRight={1}
          flexDirection="column"
          backgroundColor={theme.backgroundPanel}
        >
          <box paddingX={1} marginBottom={1}>
            <text fg={theme.text}>
              <strong>Changed Files ({files.length})</strong>
            </text>
          </box>

          <scrollbox flexGrow={1}>
            {files.map((file) => {
              const selected = selectedFile?.path === file.path;
              return (
                <box
                  key={file.path}
                  paddingX={1}
                  backgroundColor={selected ? theme.backgroundElement : "transparent"}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    setSelectedPath(file.path);
                  }}
                >
                  <text>
                    <span fg={getStatusColor(file.status, theme)}>{file.status}</span>{" "}
                    <span fg={selected ? theme.text : theme.textMuted}>{file.label}</span>
                  </text>
                </box>
              );
            })}
          </scrollbox>
        </box>

        <box flexGrow={1} border borderStyle="rounded" borderColor={theme.border} flexDirection="column" backgroundColor={theme.backgroundPanel}>
          <box paddingX={1} marginBottom={1}>
            <text fg={theme.text}>
              <strong>{selectedFile ? selectedFile.label : "No file selected"}</strong>
            </text>
          </box>

          <box flexGrow={1} padding={1}>
            {loading ? (
              <text fg={theme.textMuted}>Loading git status...</text>
            ) : error ? (
              <text fg={theme.error}>{error}</text>
            ) : !selectedFile ? (
              <text fg={theme.textMuted}>No changed files found.</text>
            ) : selectedFile.diff.trim() ? (
              <scrollbox
                flexGrow={1}
                focused
                verticalScrollbarOptions={{
                  trackOptions: {
                    backgroundColor: theme.backgroundElement,
                    foregroundColor: theme.borderActive,
                  },
                }}
              >
                <diff
                  diff={selectedFile.diff}
                  filetype={selectedFile.filetype}
                  syntaxStyle={themeBundle.syntaxStyle}
                  view="split"
                  showLineNumbers
                  width="100%"
                  wrapMode="word"
                  fg={theme.text}
                  addedBg={theme.diffAddedBg}
                  removedBg={theme.diffRemovedBg}
                  contextBg={theme.diffContextBg}
                  addedSignColor={theme.diffHighlightAdded}
                  removedSignColor={theme.diffHighlightRemoved}
                  lineNumberFg={theme.diffLineNumber}
                  lineNumberBg={theme.diffContextBg}
                  addedLineNumberBg={theme.diffAddedLineNumberBg}
                  removedLineNumberBg={theme.diffRemovedLineNumberBg}
                />
              </scrollbox>
            ) : (
              <text fg={theme.textMuted}>{selectedFile.note ?? "No diff preview available."}</text>
            )}
          </box>
        </box>
      </box>
    </box>
  );
}

const themeCatalog = await loadThemeCatalog();
const themePreference = await readThemePreferenceFromTuiConfig();

try {
  await initializeTreeSitterClient();
} catch (error) {
  console.error("Failed to initialize Tree-sitter syntax parsers:", error);
}

const initialThemeName =
  themePreference.theme && themeCatalog.themes[themePreference.theme]
    ? themePreference.theme
    : themeCatalog.themes["catppuccin-macchiato"]
      ? "catppuccin-macchiato"
      : themeCatalog.themes.opencode
        ? "opencode"
      : (themeCatalog.order[0] ?? "opencode");

const renderer = await createCliRenderer({ useMouse: true });
createRoot(renderer).render(
  <App
    themeCatalog={themeCatalog}
    initialThemeName={initialThemeName}
    initialThemeMode={themePreference.mode ?? "dark"}
  />,
);
