import { Option } from "effect";
import { resolveDiffFiletype } from "#syntax/tree-sitter";
import { FileEntry, type StatusEntry } from "#tui/types";

function normalizeStatusCode(raw: string, fallback: string): string {
	const trimmed = raw.trim();
	if (!trimmed) {
		return fallback;
	}
	return trimmed[0] ?? fallback;
}

function toStatusPair(indexCode: string, worktreeCode: string): string {
	const x = indexCode[0] ?? " ";
	const y = worktreeCode[0] ?? " ";

	if (x === "?" && y === "?") {
		return "??";
	}
	if (x === "!" && y === "!") {
		return "!!";
	}
	return `${x}${y}`;
}

function isRenameOrCopyStatus(code: string): boolean {
	return code === "R" || code === "C";
}

export function parseStatusEntries(raw: string): StatusEntry[] {
	const entries: StatusEntry[] = [];
	const fields = raw.split("\0");
	let index = 0;

	while (index < fields.length) {
		const field = fields[index];
		index += 1;

		if (!field || field.length < 4) {
			continue;
		}

		const x = field[0] ?? " ";
		const y = field[1] ?? " ";
		const status = toStatusPair(x, y);
		const firstPath = field.slice(3);

		if (!firstPath) {
			continue;
		}

		if (isRenameOrCopyStatus(x)) {
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

export function parseDiffNameStatusEntries(raw: string): StatusEntry[] {
	const entries: StatusEntry[] = [];
	const fields = raw.split("\0");
	let index = 0;

	while (index < fields.length) {
		const field = fields[index] ?? "";
		index += 1;
		if (!field) {
			continue;
		}

		const separatorIndex = field.indexOf("\t");
		const statusRaw =
			separatorIndex === -1 ? field : field.slice(0, separatorIndex);
		const inlinePath =
			separatorIndex === -1 ? "" : field.slice(separatorIndex + 1);
		const code = normalizeStatusCode(statusRaw, "M");
		const status = toStatusPair(code, " ");

		if (isRenameOrCopyStatus(code)) {
			const originalPath = inlinePath || fields[index] || "";
			if (!inlinePath) {
				index += 1;
			}
			const renamedPath = fields[index] || "";
			index += 1;

			if (!originalPath || !renamedPath) {
				continue;
			}

			entries.push({
				status,
				path: renamedPath,
				originalPath,
			});
			continue;
		}

		const path = inlinePath || fields[index] || "";
		if (!inlinePath) {
			index += 1;
		}

		if (!path) {
			continue;
		}

		entries.push({
			status,
			path,
		});
	}

	return entries;
}

export function toFileEntry(entry: StatusEntry): FileEntry {
	const label =
		entry.originalPath === undefined
			? entry.path
			: `${entry.originalPath} -> ${entry.path}`;
	const filetype = resolveDiffFiletype(entry.path);
	return FileEntry.make({
		status: entry.status,
		path: entry.path,
		label,
		...(Option.isSome(filetype) ? { filetype: filetype.value } : {}),
	});
}
