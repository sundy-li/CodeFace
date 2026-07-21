# CodeFace

[English](README.md) | [简体中文](README.zh-CN.md)

<p align="center">
  <img src="resources/app-icon/codeface-icon-1024.png" alt="CodeFace logo" width="160">
</p>


CodeFace is a native, cross-platform appearance manager for the Codex desktop app. Built with Rust and GPUI, it injects theme CSS and the required UI integration code through the Chrome DevTools Protocol (CDP), bound exclusively to the local loopback interface.

CodeFace does not modify the official Codex application, Windows installation, `app.asar`, code signatures, authentication state, or API configuration. Disabling a theme safely restores the official interface.

The standalone CodeFace app is a compact controller for opening theme management, changing CodeFace's own language and appearance, restarting Codex, or closing Codex.

### Standalone controller

![CodeFace application interface](resources/CodeFace.png)

### Theme management inside Codex

The full theme library lives directly below **Appearance** in Codex Settings. Browse local and Market themes, load full-resolution effect previews, apply or edit a theme, and open less common actions from the compact **More** menu.

![CodeFace theme management inside Codex Settings](resources/CodeFace-Settings.png)

## Install

Download the latest package from [GitHub Releases](https://github.com/sundy-li/CodeFace/releases/latest):

- **macOS 13 or later:** Download `CodeFace-macOS.zip`, extract it, and move `CodeFace.app` to Applications.
- **Windows:** Download `CodeFace-Windows.zip`, extract it, and run `CodeFace.exe`.

Release builds are currently not notarized or code-signed with a commercial certificate. On macOS, if Gatekeeper blocks the first launch, right-click `CodeFace.app`, choose **Open**, and confirm. Only download builds from this repository's Releases page.

## Features

### Manage themes

- Create, browse, preview, edit, delete, and switch themes from one place
- Import theme packs and manage them alongside built-in and locally created themes
- Search, preview, install, and apply compatible `.codex-theme` packages from the built-in [CodexThemes](https://codexthemes.ai/) Market
- Preserve market theme styling with incremental DOM compatibility, stable suggestions, and artwork-aware content positioning
- Search and preview the market in CodeFace, install or install-and-apply themes, check updates, back up and roll back themes, and export portable packages
- Verify applied themes against the live Codex DOM and automatically roll back unhealthy results
- Edit `theme.json` and `codeface.css` directly, with CSS syntax highlighting
- Open CodeFace directly below **Appearance** in Codex Settings to manage local and market themes, edit source, import or export packages, and switch between native and custom appearances

### Customize themes with an LLM

1. Create or open a theme, then upload a PNG, JPEG, or WebP background image.
2. Click **Copy full prompt** to copy the current theme directory and the canonical CodeFace Skill URL.
3. Paste the prompt into an LLM and describe the visual style you want. The LLM reads the latest Skill instructions and edits the referenced local theme directory.

CodeFace generates a plain white background automatically when no image is selected.

### More features

- Preview themes on the home screen with four native shortcut suggestions
- Close or restart Codex from the manager
- Check GitHub Latest Release from the standalone controller; verified updates download, install, and restart automatically
- English and Simplified Chinese interfaces
- Configure the standalone CodeFace window's language and light/dark appearance independently
- Keep the standalone CodeFace window focused on theme settings, its own two preferences, Codex lifecycle controls, and CodeFace updates
- One shared GPUI interface and Rust core for macOS and Windows


## Theme packs

A theme directory contains:

```text
theme.json
codeface.css
background.png
```

The template is available in [`resources/theme-pack-template`](resources/theme-pack-template). Double-click a theme in the theme list to edit its source directly.

The theme library has two searchable views: **Local** filters installed themes by name, ID, or description; **Market** loads the full CodexThemes catalog immediately, even before a search is entered, and can then filter it by style or subject. Every listing can be selected for a large local preview. Every published `kind=theme` entry can install locally, including bounded manual ZIP archives and themes without artwork. Choose **Install** to add it to the library, or **Install & apply** to install it, apply it to Codex, run the runtime health gate, and roll back automatically if verification fails. `kind=skin` listings are market design references rather than downloadable themes, so CodeFace keeps them searchable and previewable but does not mislabel them as installable packages.

Market responses are treated as external data: optional text metadata may be missing or `null` without preventing the catalog from loading. Entries without a usable theme ID or name are skipped individually, so one malformed listing cannot make the entire Market view fail. A CodexThemes skin that provides only a visual reference is labeled **Reference only** and does not show misleading install actions.

## Codex Skill

The repository includes a reusable [`codeface` Skill](skills/codeface/SKILL.md) for agents that create, import, apply, verify, repair, or restore CodeFace themes. Install the `skills/codeface` directory in your Codex Skills location, then invoke it with `$codeface`. The Skill uses CodeFace's own theme storage, Rust CLI, loopback-only CDP workflow, and runtime verification instead of introducing a separate injector.

### Built-in themes

CodeFace includes five ready-to-use theme packs. Their full source is available in [`resources/theme-packs`](resources/theme-packs), and the preview images are collected in [`resources/examples`](resources/examples).

| Theme | Preview | Description |
| --- | --- | --- |
| **Cyberpunk · Neon Skyline** | ![Cyberpunk theme preview](resources/examples/theme-cyberpunk.png) | A deep-blue cockpit interface with a neon city backdrop, cyan grid lines, and pink and amber accents. |
| **Fan Zhendong · Champion Moment** | ![Fan Zhendong theme preview](resources/examples/theme-fzd.png) | A restrained dark arena look inspired by a championship moment, with cool blue highlights and a focused portrait backdrop. |
| **Lovely Girl** | ![Lovely Girl theme preview](resources/examples/theme-lovely-girl.png) | A warm editorial style built from cream, old-paper, rose, and soft glass-like surfaces. |
| **Messi · World Champion** | ![Messi theme preview](resources/examples/theme-messi.png) | A bright commemorative theme in Argentina sky blue and trophy gold, paired with a World Cup celebration image. |
| **QQ 2007** | ![QQ 2007 theme preview](resources/examples/theme-qq2007.png) | A nostalgic compact desktop-software skin with glossy blue panels, beveled controls, and a classic QQ-inspired layout. |

## Why CDP?

Codex does not provide a complete third-party theming API. CodeFace uses the debugging protocol built into Codex's Chromium runtime to add styles at runtime instead of repackaging or modifying the official app.

Security constraints:

- The debugging port binds only to `127.0.0.1`
- WebSocket addresses must pass local host and port validation
- Theme images are limited to 16 MiB and theme CSS to 256 KiB
- Custom CSS cannot load external URLs, fonts, or `@import` rules
- Decorative layers do not intercept pointer events intended for real Codex controls

## Project structure

```text
gui/src/
├── main.rs              GPUI interface and interactions
├── i18n.rs              Language detection, persistence, and translations
├── theme.rs             Theme validation, storage, and image conversion
├── cdp.rs               Rust CDP client, validation, and injector daemon
├── paths.rs             CodeFace data directories
└── platform/
    ├── macos.rs         macOS app discovery and lifecycle
    └── windows.rs       Windows app discovery and lifecycle

resources/
├── assets/              Embedded base CSS and renderer JavaScript
├── i18n/                Embedded JSON translation catalogs
└── theme-pack-template/ Editable theme template

xtask/                   Rust packaging tool
```

The application has no runtime dependency on Shell, PowerShell, AppleScript, or external Node.js. `codeface-inject.js` is compiled into the Rust binary because DOM operations must execute inside the Codex Chromium renderer.

## Development

The current stable Rust toolchain is required.

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --locked -p codeface --target x86_64-pc-windows-gnu
cargo xtask package
```

macOS output:

```text
dist/CodeFace.app
```

Windows output:

```text
dist/windows/CodeFace.exe
```


## Data directories

- macOS: `~/Library/Application Support/CodeFace`
- Windows: `%LOCALAPPDATA%\CodeFace`

## Command-line diagnostics

The same CodeFace Rust executable provides these commands:

```text
codeface --apply-active
codeface --check-update
codeface --restart-apply-active
codeface --apply-theme cyberpunk
codeface --import-theme /path/to/theme-directory
codeface --install-codexthemes portal-panic
codeface --install-apply-codexthemes portal-panic
codeface --search-codexthemes coast
codeface --preview-codexthemes ligurian-afterglow
codeface --check-theme-update portal-panic
codeface --health-check portal-panic 9341
codeface --list-theme-backups portal-panic
codeface --rollback-theme portal-panic
codeface --export-theme portal-panic
codeface --verify 9341
codeface --inject-active 9341
codeface --start-control 9341
codeface --open-settings
codeface --restore
codeface --print-data-root
```

## Documentation

Start with [`docs/README.md`](docs/README.md) for the user guide, theme-pack format, architecture and security model, development and build instructions, CI and release process, and troubleshooting.

Contributions are welcome. Read [`CONTRIBUTING.md`](CONTRIBUTING.md), follow the [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md), and report security issues according to [`SECURITY.md`](SECURITY.md).

## Acknowledgements

CodeFace was influenced and inspired by the following post and project:

- [Randy's post on X](https://x.com/randyloop/status/2077813650564452850)
- [Fei-Away/Codex-Dream-Skin](https://github.com/Fei-Away/Codex-Dream-Skin)

## License

MIT. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md).
