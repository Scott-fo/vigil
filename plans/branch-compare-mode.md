# Branch Compare Mode Plan

## Goal
Add a PR-style review mode that compares two refs (source and destination branches), while preserving the current working-tree review flow as the default mode.

## User Experience
1. User opens a `Branch Compare` modal from the main reviewer screen.
2. Modal supports autocomplete for:
   - source ref (feature branch / head)
   - destination ref (base branch / target)
3. User confirms selection and reviewer switches into `branch-compare` mode.
4. Sidebar and diff panel render changed files and hunks for that ref range.
5. User exits compare mode with `Ctrl+L` and returns to normal working-tree mode.

## Interaction Design
- Open modal: `b` (new keybind)
- Close modal: `Esc`
- Confirm modal: `Enter`
- Move list selection: `Up/Down`
- Switch active modal field (source/destination): `Tab`
- Exit compare mode: `Ctrl+L`
- Existing `q`/`Esc` app-quit behavior remains unchanged outside modals.

## Data Semantics
- Compare command semantics should match PR review intent:
  - use `git diff <destination>...<source>`
  - this represents what source introduces relative to destination
- Compare mode is read-only:
  - disable stage/unstage
  - disable discard
  - disable commit modal

## State Model Changes
1. Introduce review mode state:
   - `working-tree` (existing behavior)
   - `branch-compare` with `{ sourceRef, destinationRef }`
2. Introduce branch compare modal state atom:
   - open/closed
   - source query + selected source ref
   - destination query + selected destination ref
   - active field
   - available refs + optional load error

## Architecture Plan

### 1. Git Data Layer
Add new helpers in `packages/tui/src/data/git.ts`:
- `listComparableRefs()` for autocomplete candidates
- `loadFilesWithBranchDiffs({ sourceRef, destinationRef })`
- parse changed file metadata from diff/status output into existing `FileEntry` shape

Keep `FileEntry` unchanged if possible to minimize UI churn.

### 2. Refresh Pipeline
Update `use-file-refresh` to refresh by review mode:
- `working-tree` -> existing `loadFilesWithDiffs()`
- `branch-compare` -> `loadFilesWithBranchDiffs(...)`

Continue polling, but poll according to the active mode.

### 3. UI State + App Wiring
Update `App` state composition to include:
- active review mode
- branch compare modal state

Render branch compare modal alongside existing modals with proper z-index/priority.

### 4. Keyboard Intents
Extend `AppKeyboardIntent` and decoder:
- open/close/confirm branch modal
- navigate branch selection
- switch modal field
- reset to working tree (`Ctrl+L`)

Ensure modal precedence remains strict (when open, only modal intents process).

### 5. Repo Actions Hook
Extend `use-repo-actions` handlers to:
- open/close/confirm branch compare modal
- apply selected refs into review mode
- reset mode to `working-tree`
- guard read/write actions based on mode

### 6. Reviewer UI
Keep existing sidebar + diff renderers.
Add small mode indicator in header:
- working tree: `Working Tree`
- branch compare: `Comparing <sourceRef> -> <destinationRef>`

## Testing Plan

### Unit Tests
1. `ui/inputs.test.ts`
   - new key intents (`b`, modal navigation, `Ctrl+L`)
   - modal precedence assertions
2. `data/git` tests
   - ref listing parsing
   - branch compare file parsing
   - rename and binary/no-diff notes handling

### Integration/Manual Checks
1. Open modal and filter refs by typing.
2. Select source/destination and apply compare mode.
3. Navigate files and verify diffs render as expected.
4. Verify staging/discard/commit actions are blocked in compare mode.
5. Press `Ctrl+L` and confirm return to working-tree data.
6. Confirm existing workflows still function unchanged.

## Rollout Plan
1. Add state + intent plumbing (no UI switch yet).
2. Add data-layer compare loaders and wire mode-based refresh.
3. Add modal UI and keyboard flow.
4. Add read-only guards + mode indicator.
5. Add tests and run full typecheck/tests.

## Risks and Mitigations
- Risk: incorrect ref orientation (`source` vs `destination`)
  - Mitigation: explicit UI labels and test fixture coverage for `destination...source`.
- Risk: large diffs can hurt responsiveness
  - Mitigation: keep existing hunk splitting; consider lazy loading if needed later.
- Risk: modal keybinding conflicts
  - Mitigation: preserve modal-first intent decoding and add targeted tests.

## Definition of Done
- User can compare two refs via modal with autocomplete.
- Diff list and file diff rendering work in compare mode.
- `Ctrl+L` reliably restores working-tree mode.
- Existing non-compare reviewer behavior remains intact.
- Tests cover new intents and compare data parsing.
