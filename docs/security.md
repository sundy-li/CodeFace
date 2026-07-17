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

## Deletion

Themes can be deleted only from `themes/<validated-id>` inside the CodeFace data directory and only after confirmation in the GUI. Deleting a theme-library copy does not modify the running Codex session or the official application.
