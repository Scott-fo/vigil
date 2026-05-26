# Diff Benchmarks

These benchmarks compare Vigil's Rust diff parser against Pierre's TypeScript
parser without installing third-party packages.

## Dependency-free Pierre baseline

Run from this repository:

```sh
bun benches/pierre_js_baseline.ts
```

The script imports Pierre's local TypeScript source from
`~/gitrepos/pierre/packages/diffs/src/utils/parsePatchFiles.ts` and uses the
same generated TSX patch fixture as `diff_performance.rs`.

It intentionally does not install dependencies. If Pierre's workspace
dependencies are absent, the script skips `parseDiffFromFile` because that
source imports the external `diff` package. The parser paths that do not need
third-party dependencies still run:

- `parsePatchFiles`
- `processFile`
- `trimPatchContext`
- `diffAcceptRejectHunk`
- `computeEstimatedDiffHeights`
- `createWindowFromScrollPosition`
- `getMergeConflictLineTypes`
- `parseMergeConflictDiffFromFile`
- `resolveConflict`

Set `PIERRE_ROOT=/path/to/pierre` to compare against a different checkout.

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
```

## Latest local results

Measured on the generated `mega-dashboard.tsx` fixture:

| Case | Implementation | Mean | Throughput |
| --- | --- | ---: | ---: |
| patch parser | Pierre `parsePatchFiles` | 0.942 ms | 664.42 MiB/s |
| patch parser | Rust `parse_patch_files` | 0.650 ms | 962.59 MiB/s |
| file parser | Pierre `processFile` | 0.756 ms | 827.30 MiB/s |
| file parser | Rust `process_file` | 0.507 ms | 1.205 GiB/s |
| patch context trim | Pierre `trimPatchContext` | 0.234 ms | 2.613 GiB/s |
| patch context trim | Rust `trim_patch_context` | 0.186 ms | 3.282 GiB/s |
| full-file diff | Rust `parse_diff_from_file` | 1.350 ms | 463.50 MiB/s |
| diff iteration | Rust `collect_diff_lines_unified` | 0.168 ms | 3.642 GiB/s |
| diff iteration | Rust `collect_diff_lines_split` | 0.168 ms | 3.646 GiB/s |

Measured on the generated 106 KiB merge-conflict fixture:

| Case | Implementation | Mean | Throughput |
| --- | --- | ---: | ---: |
| conflict line classification | Pierre `getMergeConflictLineTypes` | 0.218 ms | 463.75 MiB/s |
| conflict line classification | Rust `get_merge_conflict_line_types` | 14.69 us | 41.59 GiB/s |
| conflict parser | Pierre `parseMergeConflictDiffFromFile` | 0.286 ms | 354.29 MiB/s |
| conflict parser | Rust `parse_merge_conflict_diff_from_file` | 1.019 ms | 614.28 MiB/s |

The direct `parseDiffFromFile` JS comparison requires Pierre's already-installed
workspace dependencies. Do not install packages just for this benchmark unless
the dependency risk is acceptable for the environment.

`diff_accept_reject_hunk` is also benchmarked, but it is not used as a direct
speedup claim because the current Rust API returns owned `String` data while the
Pierre implementation rebuilds arrays of JS string references.

`resolve_conflict`/`resolveConflict` is benchmarked as a tiny resolver smoke
case. The fixture is small enough that the Pierre baseline rounds to `0.000 ms`,
so it is not used as a headline speedup claim.

`parse_merge_conflict_diff_from_file` currently prioritizes parity for the
small two-way and diff3 conflict cases. Its benchmark is included to make the
remaining optimization gap explicit.

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
| Pierre snapshot, open tree | Pierre `PathStore` | 0.621 ms |
| Pierre snapshot, open tree | Rust sidebar tree | 0.428 ms |
| Linux tree, open tree | Pierre `PathStore` | 66.625 ms |
| Linux tree, open tree | Rust sidebar tree | 83.371 ms |
| Linux tree, 32 collapsed roots | Pierre `PathStore` | 56.789 ms |
| Linux tree, 32 collapsed roots | Rust sidebar tree | 38.133 ms |
| Linux tree, search hide non-matches | Rust sidebar tree | 84.456 ms |
| viewport range math | Rust sidebar tree | 1.903 ns |

The Rust open Linux case is not yet a speedup claim; it remains benchmarked so
future tree optimizations have a concrete target.
