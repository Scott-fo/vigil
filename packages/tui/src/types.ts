import { Schema, type Option } from "effect";
import type { ThemeCatalog, ThemeMode } from "#theme/theme";

export interface StatusEntry {
	readonly status: string;
	readonly path: string;
	readonly originalPath?: string;
}

export class FileEntry extends Schema.Class<FileEntry>("FileEntry")({
	status: Schema.String,
	path: Schema.String,
	label: Schema.String,
	filetype: Schema.optional(Schema.String),
}) {
	equals(other: FileEntry): boolean {
		return (
			this.status === other.status &&
			this.path === other.path &&
			this.label === other.label &&
			this.filetype === other.filetype
		);
	}
}

export interface AppProps {
	readonly themeCatalog: ThemeCatalog;
	readonly initialThemeName: string;
	readonly initialThemeMode: ThemeMode;
	readonly chooserFilePath: Option.Option<string>;
}
