# AGENTS.md — Repository Guidelines

## Architecture

- `gui/` is the GPUI application and the only runtime executable.
- `gui/src/cdp.rs` owns the loopback CDP client and Rust injector daemon.
- `gui/src/theme.rs` owns validated, atomic theme storage and image conversion.
- `gui/src/platform/` contains isolated macOS and Windows application lifecycle implementations.
- `resources/` contains embedded CSS, renderer JavaScript, and the editable theme template.
- `xtask/` packages the native application. Runtime Shell, PowerShell, and Node launchers are not allowed.

## Commands

- `cargo test --workspace --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo check --locked -p codeface --target x86_64-pc-windows-gnu`
- `cargo xtask package`
- `target/debug/codeface --verify 9341`

## Style and Safety

Use Rust 2024 and `cargo fmt`. Keep platform-specific behavior inside `platform/macos.rs` or `platform/windows.rs`. Require strict UTF-8, atomic writes, recoverable state, bounded file sizes, and loopback-only CDP URLs. Never modify the official Codex application, `app.asar`, signatures, credentials, or API configuration. Preserve real Codex controls and keep decorative layers non-interactive.
