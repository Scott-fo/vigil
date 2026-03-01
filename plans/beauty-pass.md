# Beauty Pass Plan

## Goal

Make every file in `packages/*` and `src/*` beautiful, idiomatic TypeScript + Effect:

- minimize accidental complexity
- remove unnecessary indirection
- prefer explicit domain modeling
- use `Option` / tagged errors / `Match` where they improve clarity
- aggressively rewrite or delete code when a cleaner pattern exists

## Guardrails

- Preserve behavior unless we explicitly choose to change it.
- Keep UI interactions and keybind behavior stable.
- Run `bun run typecheck` after each tranche.
- Add/adjust focused tests when we touch non-trivial pure transforms.

## Execution Order

### 1) CLI

- [x] `src/cli/cmd/cmd.ts` (removed: unnecessary abstraction)
- [x] `src/cli/cmd/reviewer.ts` (removed: folded into entrypoint)
- [x] `src/cli/index.ts` (removed: folded into entrypoint)
- [x] `src/index.tsx`

### 2) TUI Entrypoints + Core Wiring

- [x] `packages/tui/package.json`
- [x] `packages/tui/src/index.ts`
- [x] `packages/tui/src/bootstrap.tsx`
- [x] `packages/tui/src/types.ts`

### 3) Domain/Data

- [x] `packages/tui/src/data/editor.ts`
- [x] `packages/tui/src/data/git.ts`

### 4) Syntax/Theme/Config

- [x] `packages/tui/src/config/parsers.ts` (reviewed; no rewrite needed)
- [x] `packages/tui/src/syntax/tree-sitter.ts`
- [x] `packages/tui/src/theme/theme.ts`

### 5) UI

- [x] `packages/tui/src/ui/inputs.ts` (reviewed; structure already clean)
- [x] `packages/tui/src/ui/sidebar.ts` (reviewed; pure transform kept)
- [x] `packages/tui/src/ui/app.tsx` (rewritten around extracted components + atom state module)
- [x] `packages/tui/src/ui/components/splash.tsx` (new)
- [x] `packages/tui/src/ui/components/reviewer.tsx` (new)
- [x] `packages/tui/src/ui/components/commit-modal.tsx` (new)
- [x] `packages/tui/src/ui/state.ts` (new)

### 6) Pure Transform + Tests

- [ ] `packages/tui/src/diff/hunks.ts`
- [ ] `packages/tui/src/diff/hunks.test.ts`
- [ ] `packages/tui/src/ui/sidebar.test.ts`

## Current Tranche

- In progress: Pure Transform + Tests (`packages/tui/src/{diff/hunks,ui/sidebar}.test.ts`)
