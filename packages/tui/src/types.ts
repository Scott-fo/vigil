import type { ThemeCatalog, ThemeMode } from "#theme/theme";

export interface StatusEntry {
	readonly status: string;
	readonly path: string;
	readonly originalPath?: string;
}

export interface FileEntry {
	readonly status: string;
	readonly path: string;
	readonly label: string;
	readonly diff: string;
	readonly filetype?: string;
	readonly note?: string;
}

export interface GitCommandResult {
	readonly ok: boolean;
	readonly stdout: string;
	readonly stderr: string;
}

export interface AppProps {
	readonly themeCatalog: ThemeCatalog;
	readonly initialThemeName: string;
	readonly initialThemeMode: ThemeMode;
	readonly chooserFilePath?: string;
}
