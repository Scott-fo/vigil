# Diff Benchmarks

These benchmarks compare Vigil's Rust diff parser against Pierre's TypeScript
parser through the published `@pierre/diffs` package.

## Pierre Package Baseline

Run from this repository:

```sh
pnpm install
bun benches/pierre_js_baseline.ts
```

The script imports public APIs from `@pierre/diffs` and uses the package's
compiled `dist/utils` files for benchmark helpers that are not exported from the
package root. It uses the same generated TSX patch fixture as
`diff_performance.rs`, plus a 300-file multi-diff fixture for the loading
benchmark.

Set `PIERRE_DIFFS_DIST=/path/to/@pierre/diffs/dist` to compare against a
different package build.

## Rust benchmarks

```sh
cargo bench --bench diff_performance parse_patch_files
cargo bench --bench diff_performance process_file
cargo bench --bench diff_performance trim_patch_context
cargo bench --bench diff_performance diff_accept_reject_hunk
cargo bench --bench diff_performance compute_estimated_diff_heights
cargo bench --bench diff_performance get_merge_conflict_line_types
cargo bench --bench diff_performance parse_merge_conflict_diff_from_file
cargo bench --bench diff_performance resolve_conflict
cargo bench --bench diff_performance collect_diff_lines
cargo bench --bench diff_search_performance -- --noplot
cargo bench --bench multi_diff_loading -- --noplot
cargo bench --bench highlight_registry_init -- --noplot
cargo bench --bench startup_paths -- --noplot
```

## Latest local results

Measured on the generated `mega-dashboard.tsx` fixture:

| Case | Implementation | Mean | Throughput |
| --- | --- | ---: | ---: |
| patch parser | Pierre `parsePatchFiles` | 0.917 ms | 682.21 MiB/s |
| patch parser | Rust `parse_patch_files` | 0.650 ms | 962.59 MiB/s |
| file parser | Pierre `processFile` | 0.734 ms | 851.93 MiB/s |
| file parser | Rust `process_file` | 0.507 ms | 1.205 GiB/s |
| patch context trim | Pierre `trimPatchContext` | 0.226 ms | 2.706 GiB/s |
| patch context trim | Rust `trim_patch_context` | 0.186 ms | 3.282 GiB/s |
| full-file diff | Pierre `parseDiffFromFile` | 397.819 ms | 2.04 MiB/s |
| full-file diff | Rust `parse_diff_from_file` | 1.350 ms | 463.50 MiB/s |
| diff iteration | Rust `collect_diff_lines_unified` | 0.168 ms | 3.642 GiB/s |
| diff iteration | Rust `collect_diff_lines_split` | 0.168 ms | 3.646 GiB/s |

Measured on the generated 106 KiB merge-conflict fixture:

| Case | Implementation | Mean | Throughput |
| --- | --- | ---: | ---: |
| conflict line classification | Pierre `getMergeConflictLineTypes` | 0.212 ms | 477.75 MiB/s |
| conflict line classification | Rust `get_merge_conflict_line_types` | 14.69 us | 41.59 GiB/s |
| conflict parser | Pierre `parseMergeConflictDiffFromFile` | 0.264 ms | 383.55 MiB/s |
| conflict parser | Rust `parse_merge_conflict_diff_from_file` | 0.256 ms | 2.390 GiB/s |

The Pierre package baseline includes `parseDiffFromFile` because `@pierre/diffs`
brings the required `diff` dependency into the local `pnpm` install.

`diff_accept_reject_hunk` is also benchmarked, but it is not used as a direct
speedup claim because the current Rust API returns owned `String` data while the
Pierre implementation rebuilds arrays of JS string references.

`resolve_conflict`/`resolveConflict` is benchmarked as a tiny resolver smoke
case. The fixture is small enough that the Pierre baseline rounds to `0.000 ms`,
so it is not used as a headline speedup claim.

`parse_merge_conflict_diff_from_file` follows Pierre's direct marker scanner
shape, builds resolved current/incoming contents during the scan, and caches
per-hunk unified line offsets while assembling marker rows.

# Multi-Diff Loading Benchmarks

`multi_diff_loading` measures a synthetic 300-file review scope with 96,000
diff lines. It separates the cost of parsing the whole patch, building a review
snapshot, building per-file `DiffView`s from already-parsed metadata, deriving
the search index from that snapshot, and the older per-file diff-text view path.

## Latest local multi-diff results

Measured against a 5,614,690-byte generated patch:

| Case | Implementation | Mean | Throughput |
| --- | --- | ---: | ---: |
| parse multi-file patch | Pierre `parsePatchFiles` | 15.727 ms | 340.46 MiB/s |
| parse multi-file patch | Rust `parse_patch_files` | 7.563 ms | 708.00 MiB/s |
| build review snapshot from whole patch | Rust `ReviewDiffSnapshot` | 7.510 ms | 712.99 MiB/s |
| build per-file views from parsed metadata | Rust `build_diff_view_from_file_metadata` | 54.921 ms | 97.50 MiB/s |
| build per-file views from review snapshot lookup | Rust `ReviewDiffSnapshot::build_diff_view` | 54.366 ms | 98.49 MiB/s |
| build search index from review snapshot | Rust `ReviewDiffSnapshot::build_search_index` | 2.415 ms | 2.17 GiB/s |
| build per-file views from file diff text | Rust `build_diff_view_from_diff_text` | 61.456 ms | 87.13 MiB/s |
| parse once, then build per-file views | Rust | 62.163 ms | 86.14 MiB/s |
| build one combined view from whole patch text | Rust | 67.060 ms | 79.85 MiB/s |
| estimate heights for all parsed files | Pierre `computeEstimatedDiffHeights` | 0.008 ms | 681504.81 MiB/s |

Pierre's height estimate is intentionally not a direct rendering comparison:
it is the fast layout pass that lets Pierre virtualize and defer heavier row
work. Vigil now follows the same broad shape for review-scope loading: parse
once per review snapshot, store file metadata and cheap metrics, derive the
search index from that metadata, and build only the selected/nearby `DiffView`s
from the snapshot.

# Diff Search Benchmarks

`diff_search_performance` measures a synthetic 1,000-file review scope with
1,000 added lines per file. The index is built once per review snapshot and
search reuses a `DiffSearchMatcher` across query edits.

## Latest local diff search results

Measured against 1,000,000 added diff lines:

| Case | Mean | Throughput |
| --- | ---: | ---: |
| build search index | 96.38 ms | 10.38 Melem/s |
| search top 50, common query | 150.59 ms | 6.64 Melem/s |
| search top 50, sparse query | 9.69 ms | 103.17 Melem/s |

# Startup Benchmarks

`startup_paths` measures the launch-adjacent work that blocks or competes with
the first usable diff view.

## Latest local startup results

Measured against a one-file dirty TSX repository:

| Case | Mean |
| --- | ---: |
| first-paint app state construction | 1.27 us |
| render loading first paint on `TestBackend` | 71.49 us |
| baseline `resolve_repo_root` + `load_working_tree_status` | 16.03 ms |
| current `load_status_with_repo_root` | 8.79 ms |
| selected TSX highlight registry | 41.75 ms |

`App::new` no longer awaits the fresh repository snapshot. The first paint uses
the empty loading state, then the working-tree status event queues the plain
selected diff and selected-language highlight registry work in the background.

Measured with nvim Tree-sitter highlight queries:

| Case | Mean |
| --- | ---: |
| eager prewarm, many changed filetypes | 284.94 ms |
| nearby prewarm equivalent | 92.63 ms |

Vigil no longer performs eager parser prewarm when the registry becomes ready;
adjacent highlighted diffs are primed after the selected diff has completed.

# Tree Benchmarks

These benchmarks compare Vigil's Rust sidebar tree builder with Pierre's local
`PathStore` source. They use Pierre's `packages/tree-test-data` fixtures and do
not install npm packages.

## Dependency-free Pierre tree baseline

```sh
bun benches/pierre_tree_baseline.ts
```

The script imports `~/gitrepos/pierre/packages/path-store/src/store.ts`
directly. Paths are deduplicated before benchmarking because Pierre's builder
rejects duplicate file paths.

## Rust tree benchmarks

```sh
cargo bench --bench tree_performance -- --noplot
```

## Latest local tree results

Measured against 643 Pierre snapshot paths and 92,914 Linux fixture paths:

| Case | Implementation | Mean |
| --- | --- | ---: |
| Pierre snapshot, open tree | Pierre `PathStore` | 0.634 ms |
| Pierre snapshot, open tree | Rust sidebar tree | 0.269 ms |
| Linux tree, open tree | Pierre `PathStore` | 67.851 ms |
| Linux tree, open tree | Rust sidebar tree | 46.950 ms |
| Linux tree, 32 collapsed roots | Pierre `PathStore` | 59.416 ms |
| Linux tree, 32 collapsed roots | Rust sidebar tree | 32.833 ms |
| Linux tree, search hide non-matches | Rust sidebar tree | 43.576 ms |
| viewport range math | Rust sidebar tree | 1.910 ns |

The Rust tree builder caches Pierre-style segment sort keys on nodes and files,
tracks directory change state during insertion, and uses allocation-free ASCII
search matching for common repository paths.
