const HUNK_HEADER_PATTERN = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/;

export const DIFF_GAP_EXPANSION_STEP = 20;

export interface DiffHunkBlock {
	readonly hunkIndex: number;
	readonly header: string;
	readonly headerSuffix: string;
	readonly oldStart: number;
	readonly oldCount: number;
	readonly newStart: number;
	readonly newCount: number;
	readonly bodyLines: ReadonlyArray<string>;
	readonly diff: string;
}

export interface DiffHunkGap {
	readonly previousHunkIndex: number;
	readonly nextHunkIndex: number;
	readonly oldStart: number;
	readonly oldCount: number;
	readonly newStart: number;
	readonly newCount: number;
}

export interface DiffHunkModel {
	readonly headerLines: ReadonlyArray<string>;
	readonly hunks: ReadonlyArray<DiffHunkBlock>;
	readonly gaps: ReadonlyArray<DiffHunkGap>;
}

export interface DiffGapExpansion {
	readonly fromPrevious: number;
	readonly fromNext: number;
}

function parseCount(rawCount: string | undefined): number {
	if (rawCount === undefined) {
		return 1;
	}

	const parsed = Number.parseInt(rawCount, 10);
	return Number.isNaN(parsed) ? 1 : parsed;
}

function createHunkBlock(
	hunkIndex: number,
	headerLines: ReadonlyArray<string>,
	hunkLines: ReadonlyArray<string>,
): DiffHunkBlock | null {
	const header = hunkLines[0];
	if (!header) {
		return null;
	}

	const match = header.match(HUNK_HEADER_PATTERN);
	if (!match) {
		return null;
	}

	const oldStart = Number.parseInt(match[1] ?? "0", 10);
	const newStart = Number.parseInt(match[3] ?? "0", 10);

	return {
		hunkIndex,
		header,
		headerSuffix: match[5] ?? "",
		oldStart: Number.isNaN(oldStart) ? 0 : oldStart,
		oldCount: parseCount(match[2]),
		newStart: Number.isNaN(newStart) ? 0 : newStart,
		newCount: parseCount(match[4]),
		bodyLines: hunkLines.slice(1),
		diff: [...headerLines, ...hunkLines, ""].join("\n"),
	};
}

function formatHunkRange(start: number, count: number): string {
	if (count === 1) {
		return `${start}`;
	}

	return `${start},${count}`;
}

function formatHunkHeader(
	oldStart: number,
	oldCount: number,
	newStart: number,
	newCount: number,
	headerSuffix: string,
): string {
	return `@@ -${formatHunkRange(oldStart, oldCount)} +${formatHunkRange(newStart, newCount)} @@${headerSuffix}`;
}

function formatExpandedHunkHeader(
	hunk: DiffHunkBlock,
	contextBeforeCount: number,
	contextAfterCount: number,
): string {
	return formatHunkHeader(
		Math.max(0, hunk.oldStart - contextBeforeCount),
		hunk.oldCount + contextBeforeCount + contextAfterCount,
		Math.max(0, hunk.newStart - contextBeforeCount),
		hunk.newCount + contextBeforeCount + contextAfterCount,
		hunk.headerSuffix,
	);
}

function sliceContextLines(
	fileLines: ReadonlyArray<string>,
	startLineNumber: number,
	count: number,
): ReadonlyArray<string> {
	if (count <= 0) {
		return [];
	}

	return fileLines.slice(Math.max(0, startLineNumber - 1), startLineNumber - 1 + count);
}

export function expandDiffGap(
	gap: DiffHunkGap,
	direction: "up" | "down",
	current: DiffGapExpansion | undefined,
	amount = DIFF_GAP_EXPANSION_STEP,
): DiffGapExpansion {
	const expansion = current ?? { fromPrevious: 0, fromNext: 0 };
	const remaining = Math.max(
		0,
		gap.newCount - expansion.fromPrevious - expansion.fromNext,
	);

	if (remaining === 0) {
		return expansion;
	}

	const appliedAmount = Math.min(Math.max(1, Math.floor(amount)), remaining);
	return direction === "down"
		? {
				fromPrevious: expansion.fromPrevious + appliedAmount,
				fromNext: expansion.fromNext,
			}
		: {
				fromPrevious: expansion.fromPrevious,
				fromNext: expansion.fromNext + appliedAmount,
			};
}

export function buildExpandedDiffHunkBlock(
	model: DiffHunkModel,
	hunk: DiffHunkBlock,
	fileLines: ReadonlyArray<string>,
	expansionBefore: DiffGapExpansion | undefined,
	expansionAfter: DiffGapExpansion | undefined,
): string {
	const previousGap = model.gaps.find(
		(gap) => gap.nextHunkIndex === hunk.hunkIndex,
	);
	const nextGap = model.gaps.find(
		(gap) => gap.previousHunkIndex === hunk.hunkIndex,
	);

	const contextBeforeCount = Math.min(
		previousGap?.newCount ?? 0,
		expansionBefore?.fromNext ?? 0,
	);
	const contextAfterCount = Math.min(
		nextGap?.newCount ?? 0,
		expansionAfter?.fromPrevious ?? 0,
	);
	const contextBeforeStart =
		(previousGap?.newStart ?? 0) +
		Math.max(0, (previousGap?.newCount ?? 0) - contextBeforeCount);

	const contextBeforeLines = previousGap
		? sliceContextLines(fileLines, contextBeforeStart, contextBeforeCount)
		: [];
	const contextAfterLines = nextGap
		? sliceContextLines(fileLines, nextGap.newStart, contextAfterCount)
		: [];

	return [
		...model.headerLines,
		formatExpandedHunkHeader(hunk, contextBeforeLines.length, contextAfterLines.length),
		...contextBeforeLines.map((line) => ` ${line}`),
		...hunk.bodyLines,
		...contextAfterLines.map((line) => ` ${line}`),
		"",
	].join("\n");
}

export function buildExpandedDiffHunkBlockRange(
	model: DiffHunkModel,
	hunks: ReadonlyArray<DiffHunkBlock>,
	fileLines: ReadonlyArray<string>,
	expansionBefore: DiffGapExpansion | undefined,
	expansionAfter: DiffGapExpansion | undefined,
): string {
	const firstHunk = hunks[0];
	const lastHunk = hunks[hunks.length - 1];

	if (!firstHunk || !lastHunk) {
		return "";
	}

	if (hunks.length === 1) {
		return buildExpandedDiffHunkBlock(
			model,
			firstHunk,
			fileLines,
			expansionBefore,
			expansionAfter,
		);
	}

	const previousGap = model.gaps.find(
		(gap) => gap.nextHunkIndex === firstHunk.hunkIndex,
	);
	const nextGap = model.gaps.find(
		(gap) => gap.previousHunkIndex === lastHunk.hunkIndex,
	);
	const contextBeforeCount = Math.min(
		previousGap?.newCount ?? 0,
		expansionBefore?.fromNext ?? 0,
	);
	const contextAfterCount = Math.min(
		nextGap?.newCount ?? 0,
		expansionAfter?.fromPrevious ?? 0,
	);
	const contextBeforeStart =
		(previousGap?.newStart ?? 0) +
		Math.max(0, (previousGap?.newCount ?? 0) - contextBeforeCount);
	const contextBeforeLines = previousGap
		? sliceContextLines(fileLines, contextBeforeStart, contextBeforeCount)
		: [];
	const contextAfterLines = nextGap
		? sliceContextLines(fileLines, nextGap.newStart, contextAfterCount)
		: [];
	const internalGaps = hunks.slice(0, -1).flatMap((hunk) => {
		const gap = model.gaps.find(
			(candidateGap) => candidateGap.previousHunkIndex === hunk.hunkIndex,
		);
		return gap ? [gap] : [];
	});
	const bodyLines: Array<string> = [...contextBeforeLines.map((line) => ` ${line}`)];

	hunks.forEach((hunk, index) => {
		bodyLines.push(...hunk.bodyLines);

		const internalGap = internalGaps[index];
		if (!internalGap) {
			return;
		}

		bodyLines.push(
			...sliceContextLines(fileLines, internalGap.newStart, internalGap.newCount).map(
				(line) => ` ${line}`,
			),
		);
	});

	bodyLines.push(...contextAfterLines.map((line) => ` ${line}`));
	const oldCount =
		hunks.reduce((total, hunk) => total + hunk.oldCount, 0) +
		contextBeforeLines.length +
		contextAfterLines.length +
		hunks.slice(0, -1).reduce((total, hunk) => {
			const gap = model.gaps.find(
				(candidateGap) => candidateGap.previousHunkIndex === hunk.hunkIndex,
			);
			return total + (gap?.oldCount ?? 0);
		}, 0);
	const newCount =
		hunks.reduce((total, hunk) => total + hunk.newCount, 0) +
		contextBeforeLines.length +
		contextAfterLines.length +
		hunks.slice(0, -1).reduce((total, hunk) => {
			const gap = model.gaps.find(
				(candidateGap) => candidateGap.previousHunkIndex === hunk.hunkIndex,
			);
			return total + (gap?.newCount ?? 0);
		}, 0);

	return [
		...model.headerLines,
		formatHunkHeader(
			Math.max(0, firstHunk.oldStart - contextBeforeLines.length),
			oldCount,
			Math.max(0, firstHunk.newStart - contextBeforeLines.length),
			newCount,
			firstHunk.headerSuffix,
		),
		...bodyLines,
		"",
	].join("\n");
}

export function buildDiffHunkModel(diff: string): DiffHunkModel {
	const normalized = diff.replace(/\r\n/g, "\n");
	const lines = normalized.split("\n");
	const firstHunkIndex = lines.findIndex((line) => line.startsWith("@@ "));

	if (firstHunkIndex === -1) {
		return {
			headerLines: lines,
			hunks: [],
			gaps: [],
		};
	}

	const headerLines = lines.slice(0, firstHunkIndex);
	const hunks: Array<DiffHunkBlock> = [];
	let currentHunkLines: Array<string> = [];

	for (const line of lines.slice(firstHunkIndex)) {
		if (line.startsWith("@@ ")) {
			const previousHunk = createHunkBlock(
				hunks.length,
				headerLines,
				currentHunkLines,
			);

			if (previousHunk) {
				hunks.push(previousHunk);
			}

			currentHunkLines = [line];
			continue;
		}

		if (currentHunkLines.length > 0) {
			currentHunkLines.push(line);
		}
	}

	const finalHunk = createHunkBlock(
		hunks.length,
		headerLines,
		currentHunkLines,
	);
	if (finalHunk) {
		hunks.push(finalHunk);
	}

	const gaps = hunks.flatMap((nextHunk, index) => {
		if (index === 0) {
			return [];
		}

		const previousHunk = hunks[index - 1];
		if (!previousHunk) {
			return [];
		}

		const oldStart = previousHunk.oldStart + previousHunk.oldCount;
		const newStart = previousHunk.newStart + previousHunk.newCount;
		const oldCount = Math.max(0, nextHunk.oldStart - oldStart);
		const newCount = Math.max(0, nextHunk.newStart - newStart);

		if (oldCount === 0 && newCount === 0) {
			return [];
		}

		return [
			{
				previousHunkIndex: previousHunk.hunkIndex,
				nextHunkIndex: nextHunk.hunkIndex,
				oldStart,
				oldCount,
				newStart,
				newCount,
			} satisfies DiffHunkGap,
		];
	});

	return {
		headerLines,
		hunks,
		gaps,
	};
}

export function splitDiffIntoHunkBlocks(diff: string): string[] {
	const model = buildDiffHunkModel(diff);
	return model.hunks.length === 0
		? [diff]
		: model.hunks.map((hunk) => hunk.diff);
}
