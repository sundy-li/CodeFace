# Theme-pack format

## Directory structure

```text
my-theme/
├── theme.json
├── codeface.css
└── background.png
```

All three files must exist when a theme is imported. A source theme pack may also use a JPEG or WebP image; CodeFace normalizes it to PNG when saving.

## theme.json

```json
{
  "schemaVersion": 2,
  "id": "my-theme",
  "name": "My Theme",
  "brandSubtitle": "CODEFACE CUSTOM THEME",
  "tagline": "A focused Codex workspace",
  "projectPrefix": "Project · ",
  "projectLabel": "Project",
  "statusText": "CODEFACE ONLINE",
  "quote": "MAKE SOMETHING WONDERFUL",
  "image": "background.png",
  "avatar": "avatar.png",
  "suggestions": [
    { "title": "Build", "description": "Write code and apps" },
    { "title": "Analyze", "description": "Understand code and data" },
    { "title": "Automate", "description": "Handle repeated workflows" },
    { "title": "Debug", "description": "Find and fix problems" }
  ],
  "layout": {
    "heroFit": "none",
    "heroPosition": "center center",
    "backgroundPosition": "center center",
    "taskOverlay": 0.5,
    "sidebarWidth": 260,
    "contentMaxWidth": 960,
    "composerMaxWidth": 820,
    "heroHeight": 252
  },
  "typography": {
    "fontFamily": "system-ui, sans-serif",
    "monoFontFamily": "ui-monospace, monospace",
    "fontSize": 14,
    "lineHeight": 1.5
  },
  "geometry": {
    "radiusXs": 4,
    "radiusSm": 8,
    "radiusMd": 12,
    "radiusLg": 18,
    "radiusXl": 24,
    "borderWidth": 1,
    "density": 1,
    "uiScale": 1
  },
  "effects": {
    "blur": 18,
    "saturation": 1.08,
    "shadowColor": "rgba(0,0,0,.18)",
    "shadowStrength": 1,
    "textureOpacity": 0.08,
    "motionScale": 1
  },
  "chrome": {
    "brand": true,
    "status": true,
    "quote": false,
    "particles": false,
    "orbit": false
  },
  "colors": {
    "background": "#FFFFFF",
    "panel": "#FFFFFF",
    "panelAlt": "#F7F7F7",
    "accent": "#8A9A5B",
    "accentAlt": "#A8B879",
    "secondary": "#C7B98B",
    "highlight": "#66713F",
    "text": "#343829",
    "muted": "#747866",
    "line": "#D5D3B8"
  }
}
```

`heroFit` supports `none`, `contain`, and `cover`. Positions accept safe CSS keywords or percentages. Numeric theme values are clamped to readable, bounded ranges.

`appearance.light` and `appearance.dark` may override `colors`, `typography`, `geometry`, `effects`, `layout`, and `variables` for each Codex appearance. Top-level `variables` exposes additional CSS custom properties whose names start with `--cf-`; unsafe names and values are ignored.

The v2 sections are optional and schema v1 packs remain compatible. CodeFace ships the editable `Codex 2007 · 蓝色好友面板` pack as an example of the full configuration surface.

`avatar` is optional. When present, it must name an image file in the theme directory, is limited to 4 MiB, and is exposed as `var(--codeface-avatar)` for theme CSS. Avatar data is loaded through the same local Blob URL mechanism as the background image; no external URL is permitted.

## codeface.css

Theme CSS is appended after the CodeFace base styles. Base variables can be used, for example:

```css
html.codeface main.main-surface {
  border-color: var(--cf-line) !important;
}
```

Restrictions:

- Maximum size: 256 KiB
- No `@import`
- No `@font-face`
- No `url(...)`
- Must not hide, replace, or block real Codex controls

The background image is exposed through `theme.json.image` and `var(--codeface-art)`.
