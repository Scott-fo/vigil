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

- [ ] `W2` Git domain service and typed failures
  - Files:
    - `packages/tui/src/data/git.ts`
    - `packages/tui/src/types.ts`
    - `packages/tui/src/ui/app.tsx`
  - Target:
    - Replace stringly `{ ok, error? }` with typed domain errors.
    - Centralize git command execution and decoding.
    - Keep UI-friendly projection layer at boundary.

- [ ] `W3` App state domain modeling
  - Files:
    - `packages/tui/src/ui/app.tsx`
    - `packages/tui/src/ui/inputs.ts`
  - Target:
    - Introduce `Option` for selected file/path and modal/error state where practical.
    - Reduce state transition ambiguity.

- [ ] `W4` Theme and parser loading pipeline
  - Files:
    - `packages/tui/src/theme/theme.ts`
    - `packages/tui/src/syntax/tree-sitter.ts`
    - `packages/tui/src/config/parsers.ts`
    - `packages/tui/src/bootstrap.tsx`
  - Target:
    - Typed parse/validation for config inputs.
    - Explicit recoverable/unrecoverable initialization errors.

- [ ] `W5` Pure diff/sidebar transforms
  - Files:
    - `packages/tui/src/diff/hunks.ts`
    - `packages/tui/src/ui/sidebar.ts`
  - Target:
    - Lift data transforms into pure, testable functions.
    - Add algebraic return types for edge cases.

- [ ] `W6` TUI command/intention layer
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

- In progress: `W2` Git domain service and typed failures.

## Notes

- User feedback:
  - Avoid ad-hoc `renderRepoActionError`; prefer `Effect.catchTag(s)` for error projection.
  - Prefer exhaustive, prettier tagged matching in CLI error handling.
- Applied:
  - CLI now normalizes to a tagged union and uses exhaustive `Match.tag(...).exhaustive`.
  - UI/domain error projection now uses `Effect.catchTags(...)` pattern.
  - W2 service APIs now return `Effect<Success, Error, never>` at boundaries instead of `Either`.
