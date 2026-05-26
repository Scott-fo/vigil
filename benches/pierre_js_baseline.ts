import { pathToFileURL } from 'node:url';

type FileContents = {
  name: string;
  contents: string;
  cacheKey?: string;
};

type ParseDiffFromFile = (
  oldFile: FileContents,
  newFile: FileContents,
  options?: unknown
) => unknown;

type ParsePatchFiles = (
  data: string,
  cacheKeyPrefix?: string,
  throwOnError?: boolean
) => unknown;

type ProcessFile = (
  data: string,
  options?: {
    cacheKey?: string;
    isGitDiff?: boolean;
    throwOnError?: boolean;
  }
) => unknown;

type TrimPatchContext = (patch: string, contextSize?: number) => string;
type DiffAcceptRejectHunk = (
  diff: unknown,
  hunkIndex: number,
  options: 'accept' | 'reject' | 'both'
) => unknown;
type ComputeEstimatedDiffHeights = (options: {
  fileDiff: unknown;
  metrics: unknown;
  disableFileHeader: boolean;
  hunkSeparators: 'line-info';
  expandUnchanged: boolean;
  expandedHunks: undefined;
  collapsedContextThreshold: number;
}) => unknown;
type CreateWindowFromScrollPosition = (options: {
  scrollTop: number;
  height: number;
  scrollHeight: number;
  fitPerfectly: boolean;
  fitPerfectlyOverscroll: number;
  overscrollSize: number;
}) => unknown;
type GetMergeConflictLineTypes = (lines: string[]) => unknown;
type ParseMergeConflictDiffFromFile = (
  file: FileContents,
  maxContextLines?: number
) => unknown;
type ResolveConflict = (
  diff: unknown,
  conflict: unknown,
  resolution: 'current' | 'incoming' | 'both'
) => unknown;

const PIERRE_ROOT =
  process.env.PIERRE_ROOT ?? `${process.env.HOME ?? '/Users/scottfo'}/gitrepos/pierre`;
const FILETYPE = 'tsx';
const WARMUP_MS = 1_000;
const SAMPLE_COUNT = 20;
const SAMPLE_MS = 250;

type LargeTsxFixture = {
  diff: string;
  oldFileContents: string;
  newFileContents: string;
};

function buildLargeTsxFixture(): LargeTsxFixture {
  const hunkCount = 12;
  const sectionsPerHunk = 24;
  const gapSize = 32;

  let diff =
    'diff --git a/src/ui/components/mega-dashboard.tsx b/src/ui/components/mega-dashboard.tsx\n' +
    'index 1111111..2222222 100644\n' +
    '--- a/src/ui/components/mega-dashboard.tsx\n' +
    '+++ b/src/ui/components/mega-dashboard.tsx\n';

  let oldStart = 1;
  let newStart = 1;
  const oldFileLines: string[] = [];
  const newFileLines: string[] = [];

  for (let hunkIndex = 0; hunkIndex < hunkCount; hunkIndex++) {
    while (oldFileLines.length + 1 < oldStart) {
      const lineNumber = oldFileLines.length + 1;
      oldFileLines.push(
        `const preservedOldLine${lineNumber} = \`stable-old-${lineNumber}\`;`
      );
    }
    while (newFileLines.length + 1 < newStart) {
      const lineNumber = newFileLines.length + 1;
      newFileLines.push(
        `const preservedNewLine${lineNumber} = \`stable-new-${lineNumber}\`;`
      );
    }

    const hunkLines: string[] = [];
    let oldCount = 0;
    let newCount = 0;

    for (let sectionIndex = 0; sectionIndex < sectionsPerHunk; sectionIndex++) {
      const globalIndex = hunkIndex * sectionsPerHunk + sectionIndex;

      for (const line of [
        ` import { memo, useEffect, useMemo, useState } from "react"; // section ${globalIndex}`,
        ` import type { DashboardCard, DashboardFilter, DashboardViewer } from "./types"; // section ${globalIndex}`,
        ` type DashboardSectionProps${globalIndex} = { viewer: DashboardViewer; cards: readonly DashboardCard[]; filters: readonly DashboardFilter[]; selectedId: string | null };`,
      ]) {
        hunkLines.push(` ${line}`);
        oldFileLines.push(line);
        newFileLines.push(line);
        oldCount++;
        newCount++;
      }

      for (const line of [
        `const renderLegacyCard${globalIndex} = (card: DashboardCard, selectedId: string | null) => <LegacyCard key={card.id} title={card.title} subtitle={card.subtitle} isSelected={selectedId === card.id} tags={card.tags} metric={card.metric} />;`,
        `const formatLegacyLabel${globalIndex} = (card: DashboardCard) => \`\${card.owner}:\${card.priority}:\${card.environment}:\${card.status}\`;`,
        `const buildLegacyState${globalIndex} = (cards: readonly DashboardCard[]) => cards.reduce((acc, card) => acc + (card.metric > 100 ? card.metric : 0), 0);`,
      ]) {
        hunkLines.push(`-${line}`);
        oldFileLines.push(line);
        oldCount++;
      }

      for (const line of [
        `const renderDashboardCard${globalIndex} = (card: DashboardCard, selectedId: string | null, viewer: DashboardViewer) => <DashboardCardRow key={card.id} title={card.title} subtitle={\`\${card.owner} / \${viewer.name} / \${card.status}\`} isSelected={selectedId === card.id} badges={card.tags} metric={card.metric} actions={viewer.canEdit ? ["open", "assign", "archive"] : ["open"]} />;`,
        `const formatDashboardLabel${globalIndex} = (card: DashboardCard, filters: readonly DashboardFilter[]) => \`\${card.owner}:\${card.priority}:\${card.environment}:\${card.status}:\${filters.map((filter) => filter.key).join("|")}\`;`,
        `const buildDashboardState${globalIndex} = (cards: readonly DashboardCard[], viewer: DashboardViewer) => cards.reduce((acc, card) => acc + (card.metric > 100 ? card.metric : 0) + (viewer.canEdit ? 1 : 0), 0);`,
        `const DashboardSection${globalIndex} = memo(({ viewer, cards, filters, selectedId }: DashboardSectionProps${globalIndex}) => { const visibleCards = useMemo(() => cards.filter((card) => filters.every((filter) => filter.values.includes(String(card[filter.key as keyof DashboardCard] ?? "")))), [cards, filters]); return <section data-section="${globalIndex}">{visibleCards.map((card) => renderDashboardCard${globalIndex}(card, selectedId, viewer))}</section>; });`,
      ]) {
        hunkLines.push(`+${line}`);
        newFileLines.push(line);
        newCount++;
      }

      for (const line of [
        ` export function useDashboardSection${globalIndex}(props: DashboardSectionProps${globalIndex}) {`,
        `   const [expanded${globalIndex}, setExpanded${globalIndex}] = useState<boolean>(props.selectedId !== null);`,
        `   useEffect(() => { if (props.selectedId) setExpanded${globalIndex}(true); }, [props.selectedId]);`,
      ]) {
        hunkLines.push(` ${line}`);
        oldFileLines.push(line);
        newFileLines.push(line);
        oldCount++;
        newCount++;
      }
    }

    diff += `@@ -${oldStart},${oldCount} +${newStart},${newCount} @@\n`;
    diff += `${hunkLines.join('\n')}\n`;
    oldStart += oldCount + gapSize;
    newStart += newCount + gapSize;
  }

  return {
    diff,
    oldFileContents: oldFileLines.join('\n'),
    newFileContents: newFileLines.join('\n'),
  };
}

function buildMergeConflictFixture(): string[] {
  const lines: string[] = [];
  for (let conflictIndex = 0; conflictIndex < 600; conflictIndex++) {
    lines.push(`const before${conflictIndex} = "stable";\n`);
    lines.push(`<<<<<<< HEAD conflict-${conflictIndex}\n`);
    lines.push(`const value${conflictIndex} = "ours";\n`);
    if (conflictIndex % 3 === 0) {
      lines.push(`||||||| base conflict-${conflictIndex}\n`);
      lines.push(`const value${conflictIndex} = "base";\n`);
    }
    lines.push('=======\n');
    lines.push(`const value${conflictIndex} = "theirs";\n`);
    lines.push(`>>>>>>> feature-${conflictIndex}\n`);
    lines.push(`const after${conflictIndex} = "stable";\n`);
  }
  return lines;
}

function buildMergeConflictResolutionFixture(): {
  diff: unknown;
  conflict: unknown;
} {
  return {
    diff: {
      name: 'conflict.txt',
      prevName: undefined,
      type: 'change',
      hunks: [
        {
          collapsedBefore: 0,
          splitLineCount: 3,
          splitLineStart: 0,
          unifiedLineCount: 3,
          unifiedLineStart: 0,
          additionCount: 2,
          additionStart: 1,
          additionLines: 2,
          additionLineIndex: 0,
          deletionCount: 2,
          deletionStart: 1,
          deletionLines: 2,
          deletionLineIndex: 0,
          hunkContent: [
            {
              type: 'change',
              deletions: 1,
              deletionLineIndex: 0,
              additions: 0,
              additionLineIndex: 0,
            },
            {
              type: 'context',
              lines: 1,
              additionLineIndex: 0,
              deletionLineIndex: 1,
            },
            {
              type: 'change',
              deletions: 0,
              deletionLineIndex: 2,
              additions: 1,
              additionLineIndex: 1,
            },
          ],
          hunkContext: undefined,
          hunkSpecs: '@@ -1,2 +1,2 @@\n',
          noEOFCRAdditions: false,
          noEOFCRDeletions: false,
        },
      ],
      splitLineCount: 3,
      unifiedLineCount: 3,
      isPartial: false,
      deletionLines: ['ours\n', 'base\n'],
      additionLines: ['base\n', 'theirs\n'],
      cacheKey: 'conflict-key',
    },
    conflict: {
      hunkIndex: 0,
      startContentIndex: 0,
      endContentIndex: 2,
      currentContentIndex: 0,
      baseContentIndex: 1,
      incomingContentIndex: 2,
      endMarkerContentIndex: 2,
    },
  };
}

async function importPierreParsers(): Promise<{
  parsePatchFiles: ParsePatchFiles;
  processFile: ProcessFile;
  trimPatchContext: TrimPatchContext;
  diffAcceptRejectHunk: DiffAcceptRejectHunk;
  computeEstimatedDiffHeights: ComputeEstimatedDiffHeights;
  createWindowFromScrollPosition: CreateWindowFromScrollPosition;
  getMergeConflictLineTypes: GetMergeConflictLineTypes;
  parseMergeConflictDiffFromFile: ParseMergeConflictDiffFromFile;
  resolveConflict: ResolveConflict;
  parseDiffFromFile?: ParseDiffFromFile;
}> {
  const parsePatchFilesModule = await import(
    pathToFileURL(
      `${PIERRE_ROOT}/packages/diffs/src/utils/parsePatchFiles.ts`
    ).href
  );
  let parseDiffFromFile: ParseDiffFromFile | undefined;
  try {
    const parseDiffFromFileModule = await import(
      pathToFileURL(
        `${PIERRE_ROOT}/packages/diffs/src/utils/parseDiffFromFile.ts`
      ).href
    );
    parseDiffFromFile = parseDiffFromFileModule.parseDiffFromFile;
  } catch (error) {
    console.warn(
      `Skipping pierre parseDiffFromFile: ${(error as Error).message}`
    );
  }

  return {
    parsePatchFiles: parsePatchFilesModule.parsePatchFiles,
    processFile: parsePatchFilesModule.processFile,
    trimPatchContext: (
      await import(
        pathToFileURL(
          `${PIERRE_ROOT}/packages/diffs/src/utils/trimPatchContext.ts`
        ).href
      )
    ).trimPatchContext,
    diffAcceptRejectHunk: (
      await import(
        pathToFileURL(
          `${PIERRE_ROOT}/packages/diffs/src/utils/diffAcceptRejectHunk.ts`
        ).href
      )
    ).diffAcceptRejectHunk,
    computeEstimatedDiffHeights: (
      await import(
        pathToFileURL(
          `${PIERRE_ROOT}/packages/diffs/src/utils/computeEstimatedDiffHeights.ts`
        ).href
      )
    ).computeEstimatedDiffHeights,
    createWindowFromScrollPosition: (
      await import(
        pathToFileURL(
          `${PIERRE_ROOT}/packages/diffs/src/utils/createWindowFromScrollPosition.ts`
        ).href
      )
    ).createWindowFromScrollPosition,
    getMergeConflictLineTypes: (
      await import(
        pathToFileURL(
          `${PIERRE_ROOT}/packages/diffs/src/utils/getMergeConflictLineTypes.ts`
        ).href
      )
    ).getMergeConflictLineTypes,
    parseMergeConflictDiffFromFile: (
      await import(
        pathToFileURL(
          `${PIERRE_ROOT}/packages/diffs/src/utils/parseMergeConflictDiffFromFile.ts`
        ).href
      )
    ).parseMergeConflictDiffFromFile,
    resolveConflict: (
      await import(
        pathToFileURL(
          `${PIERRE_ROOT}/packages/diffs/src/utils/resolveConflict.ts`
        ).href
      )
    ).resolveConflict,
    parseDiffFromFile,
  };
}

function runTimedLoop(durationMs: number, callback: () => unknown): {
  iterations: number;
  elapsedMs: number;
} {
  const start = performance.now();
  let iterations = 0;
  let elapsedMs = 0;
  do {
    callback();
    iterations++;
    elapsedMs = performance.now() - start;
  } while (elapsedMs < durationMs);
  return { iterations, elapsedMs };
}

function benchmark(name: string, bytes: number, callback: () => unknown): void {
  runTimedLoop(WARMUP_MS, callback);

  const samples: number[] = [];
  let iterations = 0;
  for (let index = 0; index < SAMPLE_COUNT; index++) {
    const sample = runTimedLoop(SAMPLE_MS, callback);
    iterations += sample.iterations;
    samples.push(sample.elapsedMs / sample.iterations);
  }

  samples.sort((left, right) => left - right);
  const meanMs =
    samples.reduce((sum, sample) => sum + sample, 0) / samples.length;
  const medianMs = samples[Math.floor(samples.length / 2)] ?? meanMs;
  const lowMs = samples[0] ?? meanMs;
  const highMs = samples[samples.length - 1] ?? meanMs;
  const mibPerSecond = bytes / (meanMs / 1_000) / 1024 / 1024;

  console.log(
    `${name}: mean ${meanMs.toFixed(3)} ms, median ${medianMs.toFixed(
      3
    )} ms, range ${lowMs.toFixed(3)}..${highMs.toFixed(
      3
    )} ms, throughput ${mibPerSecond.toFixed(
      2
    )} MiB/s, iterations ${iterations}`
  );
}

const {
  parsePatchFiles,
  processFile,
  trimPatchContext,
  diffAcceptRejectHunk,
  computeEstimatedDiffHeights,
  createWindowFromScrollPosition,
  getMergeConflictLineTypes,
  parseMergeConflictDiffFromFile,
  resolveConflict,
  parseDiffFromFile,
} = await importPierreParsers();
const fixture = buildLargeTsxFixture();
const mergeConflictLines = buildMergeConflictFixture();
const mergeConflictBytes = mergeConflictLines.join('').length;
const mergeConflictFile: FileContents = {
  name: 'conflicts.ts',
  contents: mergeConflictLines.join(''),
  cacheKey: 'conflicts',
};
const mergeConflictResolutionFixture = buildMergeConflictResolutionFixture();
const oldFile: FileContents = {
  name: `mega-dashboard.${FILETYPE}`,
  contents: fixture.oldFileContents,
  cacheKey: 'old',
};
const newFile: FileContents = {
  name: `mega-dashboard.${FILETYPE}`,
  contents: fixture.newFileContents,
  cacheKey: 'new',
};
const parsedPatchFile = processFile(fixture.diff, {
  cacheKey: 'bench',
  isGitDiff: true,
  throwOnError: true,
});
const virtualMetrics = {
  hunkLineCount: 2,
  lineHeight: 20,
  diffHeaderHeight: 44,
  spacing: 8,
};

console.log(`Pierre root: ${PIERRE_ROOT}`);
console.log(`Patch bytes: ${fixture.diff.length}`);
console.log(`Full-file bytes: ${oldFile.contents.length + newFile.contents.length}`);
console.log(`Merge-conflict bytes: ${mergeConflictBytes}`);

benchmark('pierre parsePatchFiles', fixture.diff.length, () =>
  parsePatchFiles(fixture.diff, 'bench', true)
);
benchmark('pierre processFile', fixture.diff.length, () =>
  processFile(fixture.diff, {
    cacheKey: 'bench',
    isGitDiff: true,
    throwOnError: true,
  })
);
benchmark('pierre trimPatchContext', fixture.diff.length, () =>
  trimPatchContext(fixture.diff, 10)
);
benchmark('pierre diffAcceptRejectHunk', fixture.diff.length, () =>
  diffAcceptRejectHunk(parsedPatchFile, 5, 'accept')
);
benchmark('pierre computeEstimatedDiffHeights', fixture.diff.length, () =>
  computeEstimatedDiffHeights({
    fileDiff: parsedPatchFile,
    metrics: virtualMetrics,
    disableFileHeader: false,
    hunkSeparators: 'line-info',
    expandUnchanged: false,
    expandedHunks: undefined,
    collapsedContextThreshold: 1,
  })
);
benchmark('pierre createWindowFromScrollPosition', 1, () =>
  createWindowFromScrollPosition({
    scrollTop: 475.25,
    height: 100,
    scrollHeight: 1000,
    fitPerfectly: false,
    fitPerfectlyOverscroll: 0,
    overscrollSize: 30,
  })
);
benchmark('pierre getMergeConflictLineTypes', mergeConflictBytes, () =>
  getMergeConflictLineTypes(mergeConflictLines)
);
benchmark('pierre parseMergeConflictDiffFromFile', mergeConflictBytes, () =>
  parseMergeConflictDiffFromFile(mergeConflictFile, 6)
);
benchmark('pierre resolveConflict', fixture.diff.length, () =>
  resolveConflict(
    mergeConflictResolutionFixture.diff,
    mergeConflictResolutionFixture.conflict,
    'incoming'
  )
);
if (parseDiffFromFile != null) {
  benchmark(
    'pierre parseDiffFromFile',
    oldFile.contents.length + newFile.contents.length,
    () => parseDiffFromFile(oldFile, newFile)
  );
}
