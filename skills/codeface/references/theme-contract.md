# Theme contract

## Files

A CodeFace theme contains:

```text
theme.json
codeface.css
background.png
```

PNG, JPEG, and WebP source images are accepted by the manager and converted into the
managed theme. Use strict UTF-8 for text files. Keep CSS at or below 256 KiB and image
input at or below 16 MiB.

## Manifest

Start from the repository's `resources/theme-pack-template/theme.json`. Preserve the
required `name`, `image`, and `colors` fields. Provide six-digit hexadecimal colors for
`background`, `panel`, `accent`, `text`, and `muted`; these values also drive the
CodeFace preview.

Choose colors with distinct roles:

- `background`: primary workspace canvas
- `panel`: sidebar, composer, and elevated surfaces
- `accent`: focus, selection, and primary action
- `text`: primary readable content
- `muted`: secondary labels that still pass on their actual surfaces

## CSS safety

- Do not use remote URLs, `@import`, network fonts, scripts, or data collection.
- Do not hide, replace, or block native Codex controls.
- Set decorative pseudo-elements and overlays to `pointer-events: none`.
- Scope selectors defensively and provide fallbacks for changing DOM structure.
- Treat `.codeface-project-bar`, `.codeface-project-section`, and
  `.codeface-composer-stack` as shared layout invariants. Themes may style their
  colors, borders, and shadows, but must not add negative margins, transforms, or
  fixed heights that can overlap the project selector and composer.
- Favor materials, borders, shadows, and restrained decoration over structural rewrites.
- Test both content-dense workspaces and narrow windows, not only the home hero.

Use `resources/theme-pack-template/codeface.css` as the canonical editable starting
point in a source checkout.
