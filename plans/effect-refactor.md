# Effect Refactor Plan

## Goals

- Replace ad-hoc `null` / `undefined` / `{ ok, error }` modeling with Effect-native data types where it improves clarity.
- Introduce explicit error ADTs at boundaries.
- Move imperative logic toward small composable workflows.
- Keep UI behavior unchanged while improving domain semantics.

## Workflow Slices

- [x] `W1` CLI boundary and argument modeling
  - Files:
    - `src/index.tsx`
    - `src/cli/index.ts`
    - `src/cli/cmd/cmd.ts`
    - `src/cli/cmd/reviewer.ts`
  - Target:
    - `Option` for optional CLI args.
    - Explicit tagged errors for parse/validation failures.
    - Clear boundary between parse -> validate -> execute.

- [x] `W2` Git domain service and typed failures
  - Files:
    - `packages/tui/src/data/git.ts`
    - `packages/tui/src/types.ts`
    - `packages/tui/src/ui/app.tsx`
  - Target:
    - Replace stringly `{ ok, error? }` with typed domain errors.
    - Centralize git command execution and decoding.
    - Keep UI-friendly projection layer at boundary.

- [x] `W3` App state domain modeling
  - Files:
    - `packages/tui/src/ui/app.tsx`
    - `packages/tui/src/ui/inputs.ts`
  - Target:
    - Introduce `Option` for selected file/path and modal/error state where practical.
    - Reduce state transition ambiguity.

- [x] `W4` Theme and parser loading pipeline
  - Files:
    - `packages/tui/src/theme/theme.ts`
    - `packages/tui/src/syntax/tree-sitter.ts`
    - `packages/tui/src/config/parsers.ts`
    - `packages/tui/src/bootstrap.tsx`
  - Target:
    - Typed parse/validation for config inputs.
    - Explicit recoverable/unrecoverable initialization errors.

- [x] `W5` Pure diff/sidebar transforms
  - Files:
    - `packages/tui/src/diff/hunks.ts`
    - `packages/tui/src/ui/sidebar.ts`
  - Target:
    - Lift data transforms into pure, testable functions.
    - Add algebraic return types for edge cases.

- [x] `W6` TUI command/intention layer
  - Files:
    - `packages/tui/src/ui/inputs.ts`
    - `packages/tui/src/ui/app.tsx`
  - Target:
    - Convert key handlers to typed command/intention values.
    - Separate key decoding from command execution.

## Execution Order

1. `W1` CLI boundary and argument modeling
2. `W2` Git domain service and typed failures
3. `W3` App state domain modeling
4. `W4` Theme and parser loading pipeline
5. `W5` Pure diff/sidebar transforms
6. `W6` TUI command/intention layer

## Current Focus

- Completed: initial Effect refactor workflow (`W1`–`W6`).

## File-by-File Queue

- [x] `F1` `packages/tui/src/ui/inputs.ts`
  - Introduce typed intent ADT for key commands (quit, navigate, stage, commit, sync, open file, scroll).
  - Decode keypress -> intent separately from executing side effects.

- [x] `F2` `packages/tui/src/ui/app.tsx`
  - Consume typed intents from `inputs.ts`.
  - Continue replacing ad-hoc nullable state with `Option` where signal is domain-meaningful.
  - Keep render behavior stable while reducing state transition ambiguity.

- [x] `F3` `packages/tui/src/types.ts`
  - Align `FileEntry` / app-facing types with Option-first domain boundaries where appropriate.
  - Keep transport/serialization types pragmatic.

- [x] `F4` `packages/tui/src/ui/sidebar.ts`
  - Keep transforms pure and deterministic.
  - Add algebraic return types for edge paths if they improve readability.

- [x] `F5` `packages/tui/src/diff/hunks.ts`
  - Keep splitting logic pure with explicit edge handling.
  - Add tests or contract comments if needed for non-obvious behavior.

## Notes

- User feedback:
  - Avoid ad-hoc `renderRepoActionError`; prefer `Effect.catchTag(s)` for error projection.
  - Prefer exhaustive, prettier tagged matching in CLI error handling.
- Applied:
  - CLI now normalizes to a tagged union and uses exhaustive `Match.tag(...).exhaustive`.
  - UI/domain error projection now uses `Effect.catchTags(...)` pattern.
  - W2 service APIs now return `Effect<Success, Error, never>` at boundaries instead of `Either`.
  - Added `packages/tui/src/data/editor.ts` with tagged errors (`EditorEnvMissingError`, `EditorLaunchError`, `ChooserWriteError`) and Effect-based workflows for chooser writes/editor launch.
  - Replaced `openSelectedFile` imperative `try/catch` in `packages/tui/src/ui/app.tsx` with `Effect.match` + tagged error rendering.
  - Replaced polling cleanup `try/finally` in `refreshFiles` with `Effect.ensuring(...)`.
  - `packages/tui/src/theme/theme.ts` now uses Effect for theme JSON loading/parsing and resolution fallback paths with tagged errors (`ThemeResolutionError`, `ThemeFile*Error`) instead of imperative `try/catch`.
  - Replaced manual `isThemeJson` guard with `Schema.decodeUnknown(ThemeJsonSchema)`; schema includes `Schema.instanceOf(RGBA)` for runtime theme objects.
  - `packages/tui/src/syntax/tree-sitter.ts` now schema-validates parser config entries and initializes client via Effect with tagged `TreeSitterInitializeError`.
  - `packages/tui/src/bootstrap.tsx` now composes startup as a single typed Effect workflow with explicit bootstrap errors and best-effort Tree-sitter initialization.
  - `packages/tui/src/data/git.ts` now carries `Option` for filetype and diff note through file-entry assembly (no `getOrUndefined`/`undefined` sentinels in that flow).
  - `packages/tui/src/ui/inputs.ts` now cleanly decodes keypresses into typed intents, and `packages/tui/src/ui/app.tsx` executes them with exhaustive `Match.tag(...).exhaustive`.
  - `packages/tui/src/ui/app.tsx` now uses `Option` for selected path and UI/modal error state transitions.
  - Added focused tests:
    - `packages/tui/src/diff/hunks.test.ts`
    - `packages/tui/src/ui/sidebar.test.ts`
