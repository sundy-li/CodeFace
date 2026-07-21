# User guide

## Starting CodeFace

Open `CodeFace.app` on macOS or launch `CodeFace.exe` on Windows. The compact controller loads its own saved language and light/dark appearance, then offers **Open Codex theme settings**, **CodeFace settings**, **Restart Codex**, **Update**, and **Close Codex**. CodeFace reads themes from the platform data directory:

- macOS: `~/Library/Application Support/CodeFace`
- Windows: `%LOCALAPPDATA%\CodeFace`

## Managing themes

Select **Open Codex theme settings** in the standalone controller, or open Codex **Settings → CodeFace**. The top of the Local list always contains the System Native theme. It has no theme injection and cannot be edited or deleted. Applying it removes only the theme layer while keeping the loopback control bridge alive, so another custom theme can be selected from Codex Settings. The explicit `--restore` diagnostic and **Close Codex** perform full cleanup instead.

Each selected custom theme provides direct **Apply** and **Edit** icon actions. Its third **More** icon opens the less-frequent actions:

- **Copy full prompt**
- **Export**
- Market update checks, update, and rollback when applicable
- **Delete**, with a confirmation before permanently removing the theme directory

Selecting a theme shows its large preview on the right; the Apply icon injects it into the current Codex session.

## CodeFace inside Codex Settings

Once Codex is running with CodeFace's loopback CDP session, open Codex **Settings** and select **CodeFace** directly below **Appearance**. The embedded page uses validated Rust theme storage and operations; renderer JavaScript does not write theme files directly.

The embedded page provides:

- Local theme search, previews, native/custom switching, and runtime health rollback
- Theme creation and source editing for `theme.json` and `codeface.css`, including optional PNG, JPEG, or WebP artwork
- **Copy full prompt**, delete, export, update checks, and rollback
- Bounded `.codex-theme` package import and bounded theme-directory import
- CodexThemes catalog search, trusted local preview, install, and install-and-apply directly from search results; reference-only skins never expose misleading install actions

The page inherits Codex's current language, colors, typography, and light/dark appearance. It has no separate CodeFace Settings tab. Codex lifecycle controls remain in the small standalone CodeFace window.

Switching to the native appearance keeps the CodeFace Settings page mounted. Selecting a custom theme from the same page restores the theme injector and runs the same live health check before reporting success.

## Creating a theme

1. Select **New Theme**.
2. Edit `theme.json` and `codeface.css`.
3. Optionally select **Choose Background Image**. If no image is selected, CodeFace saves a 1×1 plain white PNG.
4. Select **Save Theme** or **Save and Preview**.

## Copying the complete prompt

When editing an existing theme, select **Copy Complete Prompt**. CodeFace writes the following context to the system clipboard:

- The absolute path of the current theme directory
- The directory's file list
- The complete `theme.json`
- The complete `codeface.css`
- The background image path, dimensions, and file size
- Safe-editing constraints and completion checks

Paste the prompt directly into a local Codex session. Codex can use the absolute path to read and edit that theme directory.

## Standalone CodeFace window

The standalone window is a compact Codex controller rather than a second theme manager. **Open Codex theme settings** opens the embedded CodeFace page inside Codex, while **Restart Codex** and **Close Codex** control the Codex lifecycle. **CodeFace settings** contains only two preferences for the standalone window: language (system, English, or Simplified Chinese) and appearance (light or dark). These values are stored in `settings.json` and do not change Codex or the embedded theme page.

**Update** compares the embedded application version with the tag of GitHub's Latest Release. When a newer `major.minor.patch` version is available, CodeFace downloads the fixed platform asset (`CodeFace-macOS.zip` or `CodeFace-Windows.zip`) and its `.sha256` file, verifies the archive, stages the new installation, replaces the current app through a helper process, and restarts. A release is eligible for automatic update only when its tag matches the version in `gui/Cargo.toml` and both the ZIP and checksum assets are present. CodeFace restores the current installation if replacement or restart fails.

## Codex lifecycle

- **Close Codex** removes the CodeFace theme and Settings bridge, then closes Codex.
- **Restart Codex** restarts Codex in native appearance with a loopback-only CDP session and restores the Settings bridge.
- Applying a theme again reuses that CDP session and runs the live health gate.

The daemon backs off while the CDP endpoint is unavailable and reconnects when Codex returns on the same loopback port.

## Install from CodexThemes

Switch the theme library from **Local** to **Market**. CodeFace loads the full catalog immediately; search by style or subject to narrow it, then select any result to load its image in the main workspace. Every published `kind=theme` entry can be installed locally, whether it uses the standard JSON package, a bounded manual ZIP, or no artwork. Click **Install** to add it to the library, or **Install & apply** to apply it, run the runtime health gate, and automatically roll back on failure. `kind=skin` entries are previewable design references whose market pages explicitly provide creation prompts instead of downloadable theme packages. The **Local** view uses the same search area to filter installed themes by name, ID, or description.

The equivalent diagnostic commands are `--preview-codexthemes <id-or-url>`, `--install-codexthemes <id-or-url>`, and `--install-apply-codexthemes <id-or-url>`.

Only gallery entries published as installable `.codex-theme` packages are supported. Showcase skins and legacy archive packages must be handled through their market instructions.

Market themes run in a dedicated compatibility mode: their own CSS remains authoritative, CodeFace supplies current Codex semantic markers incrementally, and artwork focal points keep home controls away from the primary subject. Existing market installations can be updated in place only when their recorded CodexThemes source matches the requested theme.

## Browse and update market themes

Enter a style or subject in the CodexThemes dialog and click **Search**. Results show the theme name, author, description, and whether the item is directly installable. **Preview** loads a bounded local PNG copy of the marketplace image. Installed market themes expose **Check updates** and **Update** actions in the preview toolbar. Updating an active theme reapplies it and runs the same health gate as a manual switch.

## Health checks and automatic rollback

After every theme switch, CodeFace observes the real Codex renderer before reporting success. The report checks the expected theme ID, critical shell controls, sampled text contrast, and suggestion-tree stability. A failed report restores the previously active theme; when no previous theme is available, CodeFace restores the native appearance.

Run the same check from a terminal with `codeface --health-check <theme-id> [port]`.

## Backups, rollback, and export

CodeFace creates persistent snapshots before editing, updating, deleting, or replacing a theme during rollback. Select **Rollback** to restore the newest snapshot. The version being replaced is backed up first, so rollback itself remains reversible.

Select **Export** to create `exports/<theme-id>.codex-theme` under the CodeFace data directory. The exported schema-version-1 package contains the manifest, CSS, palette, focal point, and normalized PNG artwork.
