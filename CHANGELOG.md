# Changelog

## 1.1.1 — 2026-07-18

- Updated the Windows, dialog, WebSocket, and system-information dependencies.
- Updated GitHub Actions to current Node.js 24-compatible releases.
- Preserved the verified macOS and Windows test, package, and automatic release pipeline.

## 1.1.0 — 2026-07-17

- Added a protected system-default theme that restores the native Codex appearance.
- Reworked the preview to closely match the real Codex sidebar, home layout, suggestions, and composer.
- Moved application controls into the CodeFace header and simplified the theme-list toolbar.
- Added explicit Edit and Delete actions to every theme card, with safe deletion confirmation.
- Added full theme-context prompt copying for AI-assisted theme editing.
- Added a custom macOS title bar with always-visible close, minimize, and zoom controls.
- Added complete project documentation and GitHub CI/build workflows for macOS and Windows.

## 1.0.0 — 2026-07-17

- Initial CodeFace release.
- Native Rust and GPUI application for macOS and Windows.
- Theme creation, source editing, image import, package import, preview, and switching.
- Loopback-only Rust CDP client and persistent injection daemon.
- English and Simplified Chinese UI with system-language fallback.
- Native Codex close and restart controls.
- Rust-based cross-platform packaging through `cargo xtask package`.
