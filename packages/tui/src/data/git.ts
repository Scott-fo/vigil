import { resolveDiffFiletype } from "#syntax/tree-sitter";
import type { FileEntry, GitCommandResult, StatusEntry } from "#tui/types";

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

		if (!field || field.length < 4) {
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

export async function loadFilesWithDiffs(): Promise<{
	files: FileEntry[];
	error?: string;
}> {
	const statusResult = runGit([
		"status",
		"--porcelain=v1",
		"-z",
		"--untracked-files=all",
	]);
	if (!statusResult.ok) {
		return {
			files: [],
			error: statusResult.stderr.trim() || "Unable to run git status.",
		};
	}

	const statusEntries = parseStatusEntries(statusResult.stdout).filter(
		(entry) => entry.status !== "!!",
	);
	const files: FileEntry[] = [];

	for (const entry of statusEntries) {
		const label = entry.originalPath
			? `${entry.originalPath} -> ${entry.path}`
			: entry.path;
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
			const diffResult = runGit([
				"diff",
				"--no-color",
				"--find-renames",
				"HEAD",
				"--",
				entry.path,
			]);
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
