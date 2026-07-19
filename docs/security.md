# Security model

## Non-destructive principles

CodeFace does not modify:

- The official Codex `.app` or Windows installation directory
- `app.asar`
- Code signatures
- Login credentials or sessions
- API keys, base URLs, or model configuration
- User conversations or project data

## CDP boundary

- The launch arguments explicitly use `--remote-debugging-address=127.0.0.1`.
- HTTP discovery is fixed to `http://127.0.0.1:<port>/json/list`.
- Every WebSocket URL must use `ws`, have `127.0.0.1` or `localhost` as its host, and use exactly the expected port.
- If the default port is occupied, CodeFace selects a free port only from a bounded range.

## Theme input

- JSON must be an object containing a non-empty `name`.
- IDs are normalized, and path-traversal characters are rejected.
- CSS and images have explicit size limits.
- CSS cannot contain network resources or dynamic imports.
- Importing copies only recognized theme files.
- State and settings are written through temporary files and atomic replacement.
- CodexThemes downloads start at `https://codexthemes.ai` and may redirect only to trusted HTTPS hosts on the same domain.
- CodexThemes preview images must remain on trusted `codexthemes.ai` HTTPS hosts, are capped at 8 MiB and 8192 pixels per dimension, decoded locally, and normalized to PNG before display.
- Market packages are limited to 30 MiB, must use schema version 1, and must match the requested theme ID.
- Market artwork is decoded and normalized locally; unsupported or external CSS assets are rejected.
- An existing local theme is updated from the market only when its recorded CodexThemes source matches the requested ID.
- Theme switches are accepted only after a live-DOM health report; failed checks restore the previous theme or native appearance.
- Persistent snapshots are limited to validated theme directories and bounded by the same 30 MiB theme size limit.

## Deletion

Themes can be deleted only from `themes/<validated-id>` inside the CodeFace data directory and only after confirmation in the GUI. Deleting a theme-library copy does not modify the running Codex session or the official application.
