# User guide

## Starting CodeFace

Open `CodeFace.app` on macOS or launch `CodeFace.exe` on Windows. CodeFace reads themes from the platform data directory:

- macOS: `~/Library/Application Support/CodeFace`
- Windows: `%LOCALAPPDATA%\CodeFace`

## Managing themes

The top of the list always contains the System Native theme. It has no CodeFace injection, cannot be edited or deleted, and restores the official Codex interface while stopping the injector daemon when applied.

Each custom theme card in the left-hand theme list provides:

- **Edit**: Open the source editors for `theme.json` and `codeface.css`.
- **Delete**: Open a deletion confirmation. Confirming permanently removes the corresponding theme directory.

Double-click a theme card to open the editor. Selecting a theme shows a read-only preview on the right; selecting **Apply Theme** injects it into the current Codex session.

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

## Language settings

Open Settings and select:

- Follow system
- English
- Simplified Chinese

CodeFace follows the operating system's preferred language until an explicit choice is saved. Changes take effect immediately and are written to `settings.json`.

## Codex lifecycle

- **Close Codex** removes the CodeFace injection and closes Codex.
- **Restart Codex** removes the injection and restarts Codex in its official mode.
- Applying a theme again establishes a loopback-only CDP session.

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
