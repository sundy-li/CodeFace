# Contributing to CodeFace

Thank you for helping improve CodeFace. Bug reports, documentation fixes, translations, themes, and Rust changes are welcome.

## Before opening an issue

- Search existing issues first.
- Use the bug or feature issue template and include your operating system and CodeFace version.
- Do not post credentials, private Codex data, or security vulnerabilities in a public issue. Follow `SECURITY.md` for private reports.

## Development workflow

1. Fork the repository and create a focused branch.
2. Keep platform-specific lifecycle code in `gui/src/platform/`.
3. Add English and Simplified Chinese strings for user-facing changes.
4. Preserve loopback-only CDP URLs, bounded input sizes, strict UTF-8, atomic writes, and recoverable state.
5. Run:

   ```bash
   cargo fmt --all -- --check
   cargo test --workspace --locked
   cargo clippy --workspace --all-targets --locked -- -D warnings
   cargo xtask package
   ```

6. Open a pull request with a concise description, test evidence, and screenshots for visible UI changes.

## Theme and media contributions

Theme assets must be safe to redistribute. Do not submit copyrighted artwork, celebrity imagery, trademarks used as decoration, or other third-party media unless you have explicit redistribution rights. State the asset source and license in the pull request.

Themes must follow [`docs/theme-format.md`](docs/theme-format.md). Decorative layers must remain non-interactive and must not obscure or imitate security-sensitive Codex controls.

## Scope

CodeFace does not patch the official Codex application, `app.asar`, signatures, credentials, or API configuration. Contributions that cross those boundaries will not be accepted.
