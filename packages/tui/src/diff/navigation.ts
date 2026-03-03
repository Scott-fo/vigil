export type DiffNavigationLineKind = "context" | "add" | "remove";

export interface DiffNavigationLine {
	readonly displayIndex: number;
	readonly hunkIndex: number;
	readonly kind: DiffNavigationLineKind;
	readonly oldLine: number | null;
	readonly newLine: number | null;
	readonly content: string;
}

export interface DiffNavigationHunk {
	readonly hunkIndex: number;
	readonly header: string;
	readonly oldStart: number;
	readonly oldCount: number;
	readonly newStart: number;
	readonly newCount: number;
	readonly startDisplayIndex: number;
	readonly endDisplayIndex: number;
}

export interface DiffNavigationModel {
	readonly lines: ReadonlyArray<DiffNavigationLine>;
	readonly hunks: ReadonlyArray<DiffNavigationHunk>;
}

const HUNK_HEADER_PATTERN = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

function parseCount(rawCount: string | undefined): number {
	if (!rawCount) {
		return 1;
	}

	const parsed = Number.parseInt(rawCount, 10);
	return Number.isNaN(parsed) ? 1 : parsed;
}

export function buildDiffNavigationModel(diff: string): DiffNavigationModel {
	const lines: Array<DiffNavigationLine> = [];
	const hunks: Array<DiffNavigationHunk> = [];
	const normalized = diff.replace(/\r\n/g, "\n");
	const diffLines = normalized.split("\n");

	let currentHunkIndex = -1;
	let currentHunkHeader = "";
	let currentHunkOldStart = 0;
	let currentHunkOldCount = 0;
	let currentHunkNewStart = 0;
	let currentHunkNewCount = 0;
	let currentHunkStartDisplayIndex = 0;
	let currentHunkEndDisplayIndex = -1;
	let oldLineNumber = 0;
	let newLineNumber = 0;

	const closeCurrentHunk = () => {
		if (currentHunkIndex === -1) {
			return;
		}

		if (currentHunkEndDisplayIndex < currentHunkStartDisplayIndex) {
			return;
		}

		hunks.push({
			hunkIndex: currentHunkIndex,
			header: currentHunkHeader,
			oldStart: currentHunkOldStart,
			oldCount: currentHunkOldCount,
			newStart: currentHunkNewStart,
			newCount: currentHunkNewCount,
			startDisplayIndex: currentHunkStartDisplayIndex,
			endDisplayIndex: currentHunkEndDisplayIndex,
		});
	};

	for (const diffLine of diffLines) {
		const hunkHeader = diffLine.match(HUNK_HEADER_PATTERN);
		if (hunkHeader) {
			closeCurrentHunk();

			const oldStart = Number.parseInt(hunkHeader[1] ?? "0", 10);
			const newStart = Number.parseInt(hunkHeader[3] ?? "0", 10);

			currentHunkIndex += 1;
			currentHunkHeader = hunkHeader[0];
			currentHunkOldStart = Number.isNaN(oldStart) ? 0 : oldStart;
			currentHunkNewStart = Number.isNaN(newStart) ? 0 : newStart;
			currentHunkOldCount = parseCount(hunkHeader[2]);
			currentHunkNewCount = parseCount(hunkHeader[4]);
			currentHunkStartDisplayIndex = lines.length;
			currentHunkEndDisplayIndex = lines.length - 1;
			oldLineNumber = currentHunkOldStart;
			newLineNumber = currentHunkNewStart;
			continue;
		}

		if (currentHunkIndex === -1 || diffLine.startsWith("\\ ")) {
			continue;
		}

		const marker = diffLine[0];
		if (marker !== " " && marker !== "+" && marker !== "-") {
			continue;
		}

		const content = diffLine.slice(1);
		let kind: DiffNavigationLineKind = "context";
		let oldLine: number | null = null;
		let newLine: number | null = null;

		if (marker === "+") {
			kind = "add";
			newLine = newLineNumber;
			newLineNumber += 1;
		} else if (marker === "-") {
			kind = "remove";
			oldLine = oldLineNumber;
			oldLineNumber += 1;
		} else {
			oldLine = oldLineNumber;
			newLine = newLineNumber;
			oldLineNumber += 1;
			newLineNumber += 1;
		}

		const displayIndex = lines.length;
		lines.push({
			displayIndex,
			hunkIndex: currentHunkIndex,
			kind,
			oldLine,
			newLine,
			content,
		});
		currentHunkEndDisplayIndex = displayIndex;
	}

	closeCurrentHunk();

	return {
		lines,
		hunks,
	};
}
