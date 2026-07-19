# Changelog

## 1.5.0 — 2026-07-19

- Added first-class **Local** and **Market** library views, both with search; Market loads the full catalog on entry.
- Added in-app CodexThemes preview images with bounded, trusted-HTTPS downloads and local PNG normalization.
- Added a complete market flow for search, large preview, install, and one-step install-and-apply without leaving the theme library.
- Market install-and-apply now uses the existing runtime health gate and automatic rollback.
- Added bounded manual-ZIP compatibility and artless-theme support so every published `kind=theme` market entry can install locally; showcase skins remain preview-only references.
- Made market catalog parsing resilient to missing or `null` optional metadata, while isolating malformed entries without a usable ID or name.
- Added `--preview-codexthemes` for preview download diagnostics.

## 1.4.0 — 2026-07-19

- Added post-apply runtime health reports for theme identity, critical controls, text contrast, and suggestion stability.
- Added automatic rollback to the previous theme or native appearance when a health check fails.
- Added an in-app CodexThemes market with search results, installability guidance, installation, and applied-theme updates.
- Added market version checks and update actions for installed themes.
- Added persistent backups before edits, market updates, deletion, and rollback, with user-facing rollback controls.
- Added portable `.codex-theme` export for local and market themes.
- Added CLI commands for market search, health checks, update checks, backup listing, rollback, export, and deletion.
- Upgraded runtime state to schema 3 with stable theme IDs for verified rollback and update workflows.

## 1.3.0 — 2026-07-19

- Added direct installation of compatible `.codex-theme` packages from CodexThemes by theme ID or localized detail-page URL.
- Added a unified add-theme menu for creating local themes, importing directories, and installing market themes.
- Added validated conversion of market manifests, palettes, CSS, and artwork into the CodeFace theme library with safe same-source updates.
- Added a dedicated CodexThemes runtime mode that preserves market CSS instead of mixing it with CodeFace layout and decorative layers.
- Added semantic compatibility markers for current Codex home suggestions, project selectors, diffs, and terminals.
- Added focal-point-aware home layout positioning so controls avoid covering the primary artwork subject.
- Fixed repeated suggestion reconstruction by replacing destructive rescans with incremental DOM synchronization and removing market-theme polling.
- Added visible in-dialog installation errors and support for both `/themes/<id>` and `/zh/themes/<id>` URLs.

## 1.2.0 — 2026-07-19

- Added the CodeFace Skill and a Skill-linked prompt workflow for AI-assisted theme refinement.
- Redesigned the theme library with richer image-first cards, clearer actions, and explicit refresh and import flows.
- Added high-fidelity full-workspace previews that closely mirror the live Codex layout.
- Refined all five built-in themes with improved color balance, shared background and sidebar blending, and route-specific styling.
- Improved light-appearance readability for configuration controls and theme surfaces.
- Fixed project-selector and composer overlap with stable injected layout markers and spacing safeguards.

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
