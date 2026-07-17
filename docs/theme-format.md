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
  "schemaVersion": 1,
  "id": "my-theme",
  "name": "My Theme",
  "brandSubtitle": "CODEFACE CUSTOM THEME",
  "tagline": "A focused Codex workspace",
  "projectPrefix": "Project · ",
  "projectLabel": "Project",
  "statusText": "CODEFACE ONLINE",
  "quote": "MAKE SOMETHING WONDERFUL",
  "image": "background.png",
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
    "taskOverlay": 0.5
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

`heroFit` supports `none`, `contain`, and `cover`. `taskOverlay` is clamped to a safe range.

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
