import type { ThemeCatalog, ThemeMode } from "./theme";

export interface StatusEntry {
	status: string;
	path: string;
	originalPath?: string;
}

export interface FileEntry {
	status: string;
	path: string;
	label: string;
	diff: string;
	filetype?: string;
	note?: string;
}

export interface GitCommandResult {
	ok: boolean;
	stdout: string;
	stderr: string;
}

export interface AppProps {
	themeCatalog: ThemeCatalog;
	initialThemeName: string;
	initialThemeMode: ThemeMode;
}
