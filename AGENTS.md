# AGENTS.md

Guidance for refactoring `vigil` in the style of the local Tokio and Serde
checkouts at `~/gitrepos/tokio` and `~/gitrepos/serde`.

## Architectural North Star

Prefer deep modules: a small, named interface with substantial behavior hidden
behind it. A module should let callers think in product concepts such as
repository status, diff view, branch comparison, theme preference, modal state,
or worktree selection. If deleting a module would merely move the same
complexity into every caller, the module is earning its place. If deleting it
would simplify the system, it is probably a pass-through.

Tokio's pattern is a product-shaped surface over implementation machinery.
Serde's pattern is precise contracts between independently useful pieces. For
`vigil`, that means:

- `app` owns interactive state transitions and task lifecycle.
- `git` owns repository facts, diff parsing, and git command adapters.
- `ui` owns rendering and hit testing from already-prepared state.
- `theme` owns palette definitions and preference resolution.
- `event` and `watcher` own input and filesystem notification plumbing.

Do not let these areas casually reach through each other. Move behavior toward
the module that owns the vocabulary.

## Patterns To Copy

Observed patterns worth carrying into `vigil`:

- Facade modules: `mod` private implementation files, then re-export the few
  names callers should use.
- Product-first docs: explain the capability, constraints, cost model, and
  common usage before listing implementation detail.
- Typed contracts: use enums, option structs, and result types to make invalid
  states hard to express.
- Internal adapters: keep platform, process, filesystem, and generated-code
  machinery behind small helpers.
- Test by contract: colocated tests for local behavior, integration tests for
  multi-module behavior, regression tests for bugs that must stay fixed.
- Split only at real seams: extra crates or directories exist because they
  change build cost, dependency surface, or product ownership.

Anti-patterns to remove during cleanup:

- Large public modules that export every internal helper.
- UI code that knows git command formats or parse details.
- App state transitions spread across rendering, event handling, and git code.
- Booleans or strings that encode domain states already known to be finite.
- Tests that duplicate file layout instead of asserting behavior.

## Public Module Shape

Use `mod` plus narrow `pub use` re-exports, following Tokio modules such as
`tokio::fs` and `tokio::sync::mpsc`.

```rust
mod cache;
mod parse;
mod repo;
mod types;

pub use self::parse::{ParseDiffOptions, ParsedPatch, parse_patch_files};
pub use self::repo::{RepoStatus, load_status_for_path};
pub use self::types::{FileEntry, WorktreeEntry};
```

Keep implementation helpers private or `pub(crate)`. A caller should import the
capability, not know which helper file contains it. When a re-export list grows
too large, split by product concept before adding another flat pile of names.

Prefer modules whose names describe the product surface:

```text
git::status
git::diff
git::worktree
app::diff
app::branch_compare
ui::modals::commit_search
```

Avoid generic buckets like `helpers`, `utils`, `misc`, or `common` unless the
code is truly cross-cutting and has no product language.

When extracting from a large module, prefer this:

```rust
mod command;
mod status;
mod types;

pub use self::status::{RepoStatus, load_status};
pub use self::types::{FileEntry, FileState};
```

Over this:

```rust
pub mod command;
pub mod status;
pub mod types;
```

The first form keeps the module's interface intentional. The second makes file
layout part of the public contract.

## Documentation Style

Write module-level docs for every deep module. Tokio and Serde start modules by
explaining what the module is for, what contract it offers, what limitations or
runtime costs matter, and how to use it.

For `vigil`, a good module doc answers:

- What product concept does this module own?
- What state or invariants does it protect?
- Which operations are async, blocking, cached, or staleable?
- What should callers use first?
- What should callers not rely on?

Example:

```rust
//! Repository status and worktree queries.
//!
//! This module is the only place that shells out to `git status`, `git
//! worktree`, or related commands. Callers receive typed repository facts
//! instead of parsing command output. Results describe a point-in-time snapshot;
//! callers that display long-lived state must handle watcher refresh events.
```

Document behavior at the interface. Do not compensate for unclear structure
with comments inside the implementation.

## Types As Contracts

Follow Serde's bias toward explicit traits, enums, and option structs. Prefer
typed inputs and outputs over strings, booleans, or tuple conventions.

Use enums for finite UI and repository states:

```rust
pub enum ReviewMode {
    WorkingTree,
    CommitCompare(CommitCompareSelection),
    BranchCompare(BranchCompareSelection),
}
```

Use options structs when a function has mode flags, width/context values, or
selection behavior:

```rust
pub struct LoadDiffOptions {
    pub mode: ReviewMode,
    pub file_path: String,
    pub context_lines: usize,
}
```

Use result and error types that preserve actionable context. Tokio's channel
errors distinguish `Full` from `Closed`; `vigil` errors should distinguish
"git command failed", "path not in repo", "revision missing", and "diff parse
failed" when callers can react differently.

Prefer this:

```rust
pub enum DiffLoadError {
    GitCommand { args: Vec<String>, stderr: String },
    MissingRevision { revision: String },
    Parse { file_path: String, message: String },
}
```

Over this:

```rust
pub type DiffLoadResult<T> = Result<T, String>;
```

## Async And Blocking Work

Tokio isolates blocking filesystem work behind a small async helper. Apply the
same discipline to git commands, filesystem reads, syntax highlighting, and
large diff parsing.

- UI code should not shell out, parse raw git output, or start arbitrary tasks.
- `git` should own command execution and output parsing.
- `app` should own background task handles, request IDs, staleness checks, and
  cancellation/drop behavior.
- Expensive pure work should have a narrow async entry point when called from
  interactive flows.

Good shape:

```rust
pub async fn load_diff_view(options: LoadDiffOptions) -> Result<DiffView> {
    let snapshot = load_diff_snapshot(&options).await?;
    parse_diff_snapshot(snapshot, options.context_lines)
}
```

Avoid mixing concerns:

```rust
// Avoid: UI event handler shells out, parses text, mutates cache, and renders.
```

## Product Structure

Use extra crates only when there is a real build or product boundary. Serde
splits `serde_core`, `serde`, and `serde_derive` because compile-time and derive
dependencies are different products. Tokio keeps macros, test helpers, stream,
and utility crates separate for the same reason.

For `vigil`, stay in one crate until a split buys one of these:

- A reusable library surface independent of the TUI.
- A procedural macro or generated-code boundary.
- A test/support crate that should not ship with the app.
- A dependency-heavy module that can be optional or compiled separately.

Within the crate, prefer product directories over early crate splits.

## Testing Style

Match tests to the interface being protected.

- Put focused module tests next to parsing, state transition, and rendering
  logic when the behavior is local.
- Keep integration tests for real git behavior, filesystem behavior, and
  end-to-end review workflows.
- Add regression tests named after the behavior or bug, not the implementation
  detail.
- Use benches for known hot paths such as diff parsing, virtual line metrics,
  highlighting, and repository status loading.

Serde's test suite is exhaustive about public contracts. Tokio has separate
integration and stress-style tests for behavior that needs more than one
module. Use the same split here: small local tests for local contracts, larger
tests for repository and UI workflows.

## Refactor Checklist

Before changing a module shape, ask:

- What product concept is this module responsible for?
- What is the smallest interface callers need?
- Which invariants can move behind that interface?
- Which public names are accidental implementation details?
- Does the test exercise the interface or the current file layout?
- Would this still make sense to someone reading rustdoc first?

During cleanup, prefer a small number of deep modules over many shallow files.
Move code when it improves locality and leverage, not merely because a file is
long.
