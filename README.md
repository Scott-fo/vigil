# vigil

`vigil` is a terminal Git review tool with an IDE-style side-by-side diff UI, syntax highlighting, staging, commit/push/pull actions, and mouse support.

## Requirements

- `bun` (current project runtime/package manager)
- `git`
- terminal with mouse support enabled

## Install

### Local development

```bash
bun install
bun dev
```

### Global command (recommended while iterating)

```bash
bun run install:global (or bun install && bun link)
```

This creates a global `vigil` command using `bun link`.

## Update flow

After pulling changes:

```bash
git pull --ff-only
bun install
```

You usually do not need to re-link unless the link/bin wiring changed.

## Usage

```bash
vigil
```

Chooser mode (Helix/Yazi-style integration):

```bash
vigil --chooser-file /tmp/vigil-choice
```

When a file is opened from the sidebar, `vigil` writes the selected path to the chooser file and exits.

## Configuration

Theme/config file:

```txt
~/.local/share/vigil/tui.json
```

Environment overrides:

- `VIGIL_THEME`
- `VIGIL_THEME_MODE` (`dark` or `light`)

Legacy env vars are also accepted for compatibility:

- `REVIEWER_THEME`
- `REVIEWER_THEME_MODE`

## Helix example

```toml
[keys.normal]
A-r = [
  ":write-all",
  ":sh rm -f /tmp/vigil-chooser",
  ":insert-output sh -c 'vigil --chooser-file /tmp/vigil-chooser </dev/tty >/dev/tty 2>&1'",
  ":open %sh{cat /tmp/vigil-chooser}",
  ":redraw",
  ":reload-all",
  ":set mouse false",
  ":set mouse true",
]
```

## License

MIT. See [LICENSE](./LICENSE).

This project includes ideas/patterns derived from:

- `opencode` (MIT): https://github.com/sst/opencode
- `@opentui/*` ecosystem
