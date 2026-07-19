# Runtime workflows

## Discover

Run `codeface --print-data-root`. Theme directories live under `themes/` in the printed
root; runtime state is managed by CodeFace in the same root. Do not invent a parallel
theme store.

## Apply

Use the CodeFace UI to select and apply a theme. For an already selected active theme,
the CLI equivalent is `codeface --apply-active`. CodeFace connects only to loopback CDP
and should preserve the existing Codex session.

If CDP is unavailable, explain that CodeFace Control may need to launch or restart Codex
with its debugging port. Ask before closing or restarting the app.

## Verify

Run `codeface --health-check <theme-id> <port>` after application. A successful
operation requires a reachable loopback endpoint, the expected live theme ID, visible
critical controls, readable sampled text, and stable suggestions. `--apply-theme`
performs this check automatically and rolls back an unhealthy result. A toast, saved
state file, running daemon, or newly written CSS is insufficient proof by itself.

When verification fails, inspect in this order:

1. the exact loopback `/json/version` endpoint;
2. the configured port and WebSocket host validation;
3. the selected theme's manifest, image, and CSS validation;
4. CodeFace runtime state, including whether injection is enabled;
5. the DOM marker after applying again.

## Restore

Run `codeface --restore`. This removes CodeFace-owned injected styles and renderer
artifacts while leaving official Codex files and the CDP session untouched. Verify the
native state instead of restarting by default.

## Package

Use the CodeFace UI import action for portable packs, or import an already extracted
directory with `codeface --import-theme <directory>`. Never extract untrusted archives
with unrestricted paths; rely on CodeFace's validated, bounded, atomic storage path.

Search the market with `codeface --search-codexthemes <query>`, cache a bounded
preview with `--preview-codexthemes <id-or-url>`, install with
`--install-codexthemes <id-or-url>`, install and immediately verify with
`--install-apply-codexthemes <id-or-url>`, and check versions with
`--check-theme-update <id>`. CodeFace snapshots an existing market copy before update.
Use `--list-theme-backups <id>` and `--rollback-theme <id>` for history, and
`--export-theme <id>` to create a portable `.codex-theme` package.
