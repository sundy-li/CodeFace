---
name: codeface
description: Create, import, edit, apply, switch, verify, repair, or restore themes for the official Codex desktop app with the CodeFace native manager. Use when a user mentions CodeFace, asks to customize Codex appearance with a local image or visual brief, provides a CodeFace theme pack, wants to diagnose a CodeFace theme that did not take effect, or wants to return Codex to its native appearance on macOS or Windows.
---

# CodeFace

Use CodeFace as the sole theme runtime. Never modify the official Codex app,
`app.asar`, signatures, credentials, or API configuration.

## Locate CodeFace

1. Prefer an installed `CodeFace` application or `codeface` executable.
2. In a CodeFace source checkout, use `cargo run -p codeface --` for CLI work.
3. Run `--print-data-root` to discover the managed data directory; never guess it.
4. Read [references/theme-contract.md](references/theme-contract.md) before creating
   or editing a theme. Read [references/runtime-workflows.md](references/runtime-workflows.md)
   before applying, restoring, or diagnosing one.

## Choose the workflow

- **Create or redesign:** derive a concise design contract from the user's image or
  brief, create a theme in the CodeFace data root, then validate and preview it.
- **Import:** use the CodeFace UI import action or `--import-theme <directory>` for a
  user-provided CodeFace theme pack.
- **Apply or switch:** use `--apply-theme <id>`, preserve the current Codex session,
  and hot-apply over loopback CDP whenever it is already available. Ask before
  restarting Codex.
- **Repair:** inspect the actual theme files, runtime state, CDP endpoint, and DOM
  verification result. Do not treat a saved file or success toast as proof.
- **Restore:** run `--restore`; keep the existing CDP session available and verify
  that CodeFace injection is disabled.

## Create a theme

1. Inspect the supplied image or brief. Choose background composition, surface
   material, accent, text contrast, and decoration density without asking users for
   internal CSS terminology.
2. Keep the native Codex information architecture and real controls intact.
3. Use an original or user-authorized local image. Do not introduce remote assets,
   fonts, `@import`, or external URLs.
4. Edit only `theme.json`, `codeface.css`, and the local background image inside the
   selected theme directory.
5. Keep decorative layers non-interactive and ensure conversations, settings, menus,
   diffs, terminal, composer, sidebar states, and narrow windows remain readable.
6. Preview before applying. State which theme will be changed and whether a Codex
   restart would be needed.

## Apply and verify

Ask before changing live Codex state unless the user explicitly requested application.
Prefer hot application; never restart merely to refresh a theme. After applying, run:

```bash
codeface --verify 9341
```

Use the configured port if it differs. Report success only after the endpoint and
CodeFace marker are both verified. Always tell the user that the native appearance can
be restored with:

```bash
codeface --restore
```

## Source development

When modifying CodeFace itself, preserve platform boundaries and run the relevant
gates from the repository root:

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

For packaging, use `cargo xtask package`. Validate native Windows packaging on a
Windows runner; a cross-target `cargo check` is compile coverage only.
