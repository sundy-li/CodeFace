# Troubleshooting

## Codex is running without CDP

CodeFace does not forcibly close an existing Codex process without authorization. Save any input, close Codex, and apply the theme again, or use CodeFace's restart action to establish a new session.

## The theme disappears after being applied

Run:

```text
codeface --verify 9341
```

The state file is stored in the platform-specific CodeFace data directory. Confirm that `injector_pid` identifies CodeFace's own `--injector-daemon` process.

## A theme cannot be imported

Confirm that the directory contains:

- `theme.json`
- `codeface.css`
- The image referenced by the JSON `image` field

Also check the JSON syntax, CSS size, and prohibited external resources.

## The background image is not displayed

Confirm that the image can be decoded as PNG, JPEG, or WebP, that it is smaller than 16 MiB, and that the saved `theme.json.image` value is `background.png`.

## Restore the official interface

```text
codeface --restore
```

Alternatively, close or restart Codex from the GUI. Restoration removes only the live injection and daemon state; it does not modify the user's theme library.
