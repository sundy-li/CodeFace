# Development and build

## Requirements

- Stable Rust with edition 2024 support
- macOS 13+ or Windows
- The system `codesign` tool for macOS packaging
- A Windows runner or an installed Rust Windows target for Windows builds

## Common commands

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Verify the Windows compilation boundary from macOS:

```bash
rustup target add x86_64-pc-windows-gnu
cargo check --locked -p codeface --target x86_64-pc-windows-gnu
```

## Packaging

```bash
cargo xtask package
```

`xtask` produces the following output for the current platform:

- macOS: `dist/CodeFace.app`
- Windows: `dist/windows/CodeFace.exe`

## Diagnostic commands

```text
codeface --apply-active
codeface --verify 9341
codeface --restore
codeface --print-data-root
```

## Change checklist

- Add both English and Simplified Chinese text for UI changes.
- Keep platform-specific behavior in `gui/src/platform/`.
- Update the template and `theme-format.md` when the theme format changes.
- For CDP changes, verify loopback restrictions, application, restoration, and survival across navigation.
- For UI changes, check the theme list, editor, settings page, and deletion confirmation.
