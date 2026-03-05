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

### Global command (compiled install)

```bash
bun install
bun run build:install
```

This installs:

- binary: `~/.local/lib/vigil/vigil`
- wrapper: `~/.local/bin/vigil`

If `vigil` is not found, add `~/.local/bin` to your `PATH`.

## Update flow

After pulling changes:

```bash
git pull --ff-only
bun install
bun run build:install
```

Re-run `build:install` whenever you want to refresh the global binary.

## Usage

```bash
vigil
```

Blame mode:

```bash
vigil blame <file>:<line>
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

# Open vigil blame for the current file + cursor line.
A-b = [
  ":write-all",
  ":insert-output sh -c 'vigil blame \"%{buffer_name}:%{cursor_line}\" </dev/tty >/dev/tty 2>&1'",
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
