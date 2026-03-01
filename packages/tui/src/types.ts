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
	diff: Schema.String,
	filetype: Schema.optional(Schema.String),
	note: Schema.optional(Schema.String),
}) {
	equals(other: FileEntry): boolean {
		return (
			this.status === other.status &&
			this.path === other.path &&
			this.label === other.label &&
			this.diff === other.diff &&
			this.filetype === other.filetype &&
			this.note === other.note
		);
	}
}

export interface AppProps {
	readonly themeCatalog: ThemeCatalog;
	readonly initialThemeName: string;
	readonly initialThemeMode: ThemeMode;
	readonly chooserFilePath: Option.Option<string>;
}
