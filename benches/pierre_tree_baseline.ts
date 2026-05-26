import { readFileSync } from 'node:fs';

const PIERRE_ROOT =
  process.env.PIERRE_ROOT ?? `${process.env.HOME ?? '/Users/scottfo'}/gitrepos/pierre`;

const { PathStore } = await import(
  `${PIERRE_ROOT}/packages/path-store/src/store.ts`
);

type LinuxFixture = {
  files: string[];
};

type BenchResult = {
  label: string;
  meanMs: number;
  iterations: number;
};

function readJson<T>(path: string): T {
  return JSON.parse(readFileSync(path, 'utf8')) as T;
}

function uniquePaths(paths: string[]): string[] {
  return Array.from(new Set(paths));
}

function benchmark(label: string, fn: () => unknown): BenchResult {
  const warmupUntil = performance.now() + 500;
  while (performance.now() < warmupUntil) {
    fn();
  }

  const samples: number[] = [];
  let totalIterations = 0;
  for (let sample = 0; sample < 20; sample += 1) {
    const start = performance.now();
    let iterations = 0;
    while (performance.now() - start < 250) {
      fn();
      iterations += 1;
    }
    const elapsed = performance.now() - start;
    samples.push(elapsed / iterations);
    totalIterations += iterations;
  }

  const meanMs = samples.reduce((sum, value) => sum + value, 0) / samples.length;
  const sorted = [...samples].sort((a, b) => a - b);
  const medianMs = sorted[Math.floor(sorted.length / 2)] ?? meanMs;
  console.log(
    `${label}: mean ${meanMs.toFixed(3)} ms, median ${medianMs.toFixed(
      3
    )} ms, iterations ${totalIterations}`
  );
  return { label, meanMs, iterations: totalIterations };
}

const treeDataRoot = `${PIERRE_ROOT}/packages/tree-test-data`;
const pierrePaths = uniquePaths(
  readJson<string[]>(`${treeDataRoot}/pierre-snapshot-files.json`)
);
const linuxPaths = uniquePaths(
  readJson<LinuxFixture>(`${treeDataRoot}/linux-files.json`).files
);
const collapsedRoots = Array.from(
  new Set(
    linuxPaths
      .filter((path) => path.includes('/'))
      .map((path) => path.split('/')[0])
      .filter((root): root is string => root != null && root.length > 0)
      .map((root) => `${root}/`)
  )
)
  .sort()
  .slice(0, 32);

function build(paths: string[], extra: Record<string, unknown> = {}) {
  const store = new PathStore({
    flattenEmptyDirectories: true,
    initialExpansion: 'open',
    paths,
    ...extra,
  });
  return store.getVisibleSlice(0, store.getVisibleCount() - 1).length;
}

function buildWithCollapsed(paths: string[], collapsedPaths: string[]) {
  const store = new PathStore({
    flattenEmptyDirectories: true,
    initialExpansion: 'open',
    paths,
  });
  for (const path of collapsedPaths) {
    store.collapse(path);
  }
  return store.getVisibleSlice(0, store.getVisibleCount() - 1).length;
}

console.log(`Pierre root: ${PIERRE_ROOT}`);
console.log(`Pierre snapshot paths: ${pierrePaths.length}`);
console.log(`Linux paths: ${linuxPaths.length}`);

benchmark('pierre path-store build_pierre_snapshot_open', () => build(pierrePaths));
benchmark('pierre path-store build_linux_open', () => build(linuxPaths));
benchmark('pierre path-store build_linux_with_collapsed_roots', () =>
  buildWithCollapsed(linuxPaths, collapsedRoots)
);
