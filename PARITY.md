# Vigil Parity Tracker

This document tracks user-visible parity between this Rust `ratatui` rewrite and the older `../reviewer` project.

The goal is not architectural parity. The old daemon and `serve` mode existed to support a direction we no longer want. For now, parity means: if a feature matters in day-to-day terminal Git review, it belongs here.

## Parity Definition

We should treat the Rust app as "good enough to replace reviewer" when it covers:

- working tree review
- commit and branch comparison
- blame flow
- common Git actions
- editor / chooser integration
- theme and config behavior
- auto-refresh on repo changes
- the small UX pieces that make the tool feel complete

Explicit non-goals right now:

- [ ] background daemon
- [ ] `serve` mode
- [ ] watcher multiplexing across clients
- [ ] AI / RPC features

## Current Rust Status

Already implemented:

- [x] Changed-files sidebar with compressed directory tree
- [x] Keyboard sidebar navigation
- [x] Mouse click to select files in the sidebar
- [x] Unified diff mode
- [x] Split diff mode
- [x] Tree-sitter syntax highlighting for installed grammars
- [x] Mouse wheel always scrolls the diff pane
- [x] `Ctrl-D` / `Ctrl-U` diff scrolling
- [x] Single-column unified line numbers
- [x] Full-width add/remove background in unified mode
- [x] Visible separator row between hunks
- [x] Catppuccin Macchiato-style palette approximation
- [x] Selected diff-line cursor, highlight, and open-at-line flow
- [x] Stage / unstage
- [x] Discard changes
- [x] Commit flow
- [x] Pull / push with remote-sync status
- [x] Open with editor
- [x] Suspend / restore TUI cleanly around editor launch
- [x] Watcher / auto-refresh with debounce and Git-aware ignore filtering
- [x] Snackbar / transient notifications

Missing from the Rust app:

- [x] theme picker and persistence
- [x] commit history search
- [x] open specific commit
- [x] branch compare
- [ ] blame mode
- [ ] chooser-file integration
- [x] help modal and current keybinding coverage
- [ ] richer error overlays / surfacing

## Feature Checklist

### 1. CLI Entry Points

- [ ] `vigil`
- [ ] `vigil blame <file>:<line>`
- [ ] `vigil --chooser-file <path>`
- [ ] `-h` / `--help`

Notes:

- Old `reviewer` also supports `vigil serve`, but that is intentionally out of scope for this rewrite.
- `blame` is not a separate app mode in practice; it boots the TUI with an initial blame target.

Source cues:

- `../reviewer/README.md`
- `../reviewer/src/cli-args.ts`

### 2. Core Review Surface

- [x] Changed-files sidebar tree
- [x] Sidebar ordering matches rendered tree order
- [x] File selection by keyboard
- [x] File selection by mouse
- [x] Unified diff view
- [x] Split diff view
- [x] Syntax highlighting in diff content
- [x] Hunk separation in rendered diff
- [x] Selected diff-line navigation
- [x] Selected diff-line highlight
- [x] Open selected diff line in editor
- [ ] Sidebar directory collapse / expand
- [ ] Split-mode context expansion between hunks
- [ ] Reviewer-style diff navigation behavior

Source cues:

- `../reviewer/packages/tui/src/ui/components/reviewer.tsx`
- `../reviewer/packages/tui/src/ui/hooks/use-diff-preview-state.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-diff-expansion-state.ts`
- `../reviewer/packages/tui/src/diff/navigation.ts`
- `../reviewer/packages/tui/src/diff/hunks.ts`

### 3. Working Tree Git Actions

- [x] Stage selected file
- [x] Unstage selected file
- [x] Discard selected file changes
- [x] Commit staged changes
- [x] Pull from remote
- [x] Push to remote
- [ ] Initialize Git repo from splash state
- [x] Remote sync running state
- [x] Read-only behavior when in branch/commit compare modes

Notes:

- In `reviewer`, stage/unstage is bound to `space` on the selected sidebar file.
- Commit only opens when there are staged files.
- Discard is a modal-confirmed destructive action.

Source cues:

- `../reviewer/packages/tui/src/data/git/status.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-git-actions.ts`
- `../reviewer/packages/tui/src/ui/components/commit-modal.tsx`
- `../reviewer/packages/tui/src/ui/components/discard-modal.tsx`
- `../reviewer/packages/tui/src/ui/components/remote-sync-status.tsx`

### 4. Commit / History Review

- [x] Commit search modal
- [x] Search by commit hash
- [x] Search by short hash
- [x] Search by subject text
- [x] Keyboard selection inside commit search
- [x] Open selected commit in compare mode
- [x] Reset back to working tree mode

Notes:

- In `reviewer`, commit search is a primary workflow, not a hidden extra. This should not be treated as optional polish.

Source cues:

- `../reviewer/packages/tui/src/data/git/compare.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-commit-search-actions.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-commit-search-view.ts`
- `../reviewer/packages/tui/src/ui/components/commit-search-modal.tsx`

### 5. Branch Compare

- [x] Branch compare modal
- [x] Source ref selection
- [x] Destination ref selection
- [x] Ref search/filter
- [x] Keyboard navigation inside modal
- [x] Open branch compare mode
- [x] Reset back to working tree mode

Notes:

- This is a separate modal and flow from commit search.
- `reviewer` treats branch compare as a first-class review mode with its own file list and diff loading behavior.

Source cues:

- `../reviewer/packages/tui/src/data/git/compare.ts`
- `../reviewer/packages/tui/src/data/git/preview.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-branch-compare-actions.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-branch-compare-view.ts`
- `../reviewer/packages/tui/src/ui/components/branch-compare-modal.tsx`

### 6. Blame Flow

- [ ] Initial blame target from CLI
- [ ] Blame overlay / panel
- [ ] Commit hash, author, date, subject, description display
- [ ] Uncommitted-line special case
- [ ] Open commit compare from blamed line when available
- [ ] Scroll blame view independently

Notes:

- This is more than `git blame` output. The old app resolves the blamed commit, loads commit metadata, and offers a jump into commit compare mode.

Source cues:

- `../reviewer/packages/tui/src/data/git/blame.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-blame-view.ts`
- `../reviewer/packages/tui/src/ui/components/blame-view.tsx`

### 7. Theme / Config

- [x] Theme picker modal
- [x] Theme search/filter
- [x] Apply selected theme immediately
- [x] Persist selected theme
- [x] Support `~/.local/share/vigil/tui.json`
- [x] Support `VIGIL_THEME`
- [x] Support `VIGIL_THEME_MODE`
- [x] Dark/light mode switching

Notes:

- Theme parity is not just colors. It also includes the config story and env-var compatibility.

Source cues:

- `../reviewer/README.md`
- `../reviewer/packages/tui/src/theme/theme.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-theme-actions.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-theme-view.ts`
- `../reviewer/packages/tui/src/ui/components/theme-modal.tsx`

### 8. Editor / External Integration

- [x] Open selected sidebar file in `$VISUAL` / `$EDITOR`
- [x] Open selected diff line in editor at line number
- [x] Suspend and restore TUI cleanly around editor launch
- [ ] Chooser-file write-and-exit mode
- [ ] Preserve chooser behavior from sidebar open action

Notes:

- In chooser mode, opening a file writes the selected path and exits instead of launching an editor.
- Editor integration is part of the replacement bar because it is a fast path back into the main editor.

Source cues:

- `../reviewer/packages/tui/src/data/editor.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-git-actions.ts`
- `../reviewer/README.md`

### 9. Auto Refresh / Watching

- [x] Watch repo for file changes
- [x] Debounce bursts of events
- [x] Ignore `.git`
- [x] Ignore Git-ignored paths where appropriate
- [ ] Snapshot-based refresh instead of blind redraw on every event
- [ ] Safety polling if watch stream dies or misses events
- [x] Refresh working-tree file list automatically
- [x] Refresh active diff automatically when current file changes

Notes:

- We should keep this simpler than the old daemon model.
- Recommended Rust shape:
- watcher callback sends a cheap "repo changed" message
- app runtime debounces and recomputes snapshots
- UI thread never does heavy work in the watch callback

Source cues:

- `../reviewer/packages/server/src/repo-watcher.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-daemon-watch.ts`
- `../reviewer/packages/tui/src/ui/hooks/use-file-refresh.ts`

### 10. UI Chrome / Completion Work

- [x] Help modal
- [x] Keybinding discoverability
- [x] Snackbar / transient notifications
- [ ] Error display
- [ ] Splash state
- [ ] "not a git repo" flow
- [ ] Modal focus and dismissal rules
- [x] Remote-sync status indicator

Notes:

- These are easy to dismiss as polish, but `reviewer` uses them to make the app understandable and safe.

Source cues:

- `../reviewer/packages/tui/src/ui/components/help-modal.tsx`
- `../reviewer/packages/tui/src/ui/components/snackbar.tsx`
- `../reviewer/packages/tui/src/ui/components/splash.tsx`
- `../reviewer/packages/tui/src/ui/components/global-overlays.tsx`
- `../reviewer/packages/tui/src/ui/state.ts`

## Keybinding Parity Worth Tracking

These are worth carrying over because they shape the feel of the app:

- [x] `space` to stage / unstage selected file
- [x] `v` to toggle unified / split diff
- [x] `tab` to switch pane focus
- [x] `Ctrl-D` / `Ctrl-U` to scroll diff half pages
- [x] `c` to open commit modal
- [x] `d` to open discard modal
- [x] `t` to open theme modal
- [x] `b` to open branch compare modal
- [x] `g` to open commit search modal
- [x] `p` to pull
- [x] `P` to push
- [x] `?` to open help
- [x] `e` / `o` / `enter` to open in editor
- [x] `Ctrl-L` to reset compare mode back to working tree
- [ ] `Ctrl-B` to toggle sidebar
- [ ] `Ctrl-W h` / `Ctrl-W l` for pane focus parity if we still want pane focus as a concept

Source cue:

- `../reviewer/packages/tui/src/ui/inputs.ts`

## Suggested Build Order

Recommended order from here:

1. Blame
   Build on top of commit compare once revision review is solid.

2. Safety polling for watcher
   This is the main remaining reliability gap in the watch flow.

3. Help, splash, and finishing UX
   These should land before calling the rewrite a replacement.
