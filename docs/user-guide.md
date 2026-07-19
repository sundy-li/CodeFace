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

Open the **+** menu in the theme library, click **Install from CodexThemes**, enter a published theme ID or a `https://codexthemes.ai[/zh]/themes/<id>` detail-page URL, and click **Install**. CodeFace uses the official anonymous download endpoint, validates the `.codex-theme` package, converts its manifest, CSS, palette, and artwork into the CodeFace theme layout, and selects the installed theme. Applying it to Codex remains a separate explicit action.

Only gallery entries published as installable `.codex-theme` packages are supported. Showcase skins and legacy archive packages must be handled through their market instructions.

Market themes run in a dedicated compatibility mode: their own CSS remains authoritative, CodeFace supplies current Codex semantic markers incrementally, and artwork focal points keep home controls away from the primary subject. Existing market installations can be updated in place only when their recorded CodexThemes source matches the requested theme.
