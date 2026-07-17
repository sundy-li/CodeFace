# Native Rust architecture

CodeFace is one GPUI executable with embedded theme assets. The UI calls Rust services directly; there are no runtime platform scripts or Node processes.

```text
GPUI UI
  ├─ I18n: persisted language preference and system-locale fallback
  ├─ ThemeStore: validated JSON, atomic writes, image normalization
  ├─ CdpEngine: loopback discovery, WebSocket protocol, injection and verification
  ├─ Injector daemon: the same executable in a background mode
  └─ PlatformBackend
       ├─ macOS: app discovery and process lifecycle
       └─ Windows: executable discovery and process lifecycle
```

The injected JavaScript remains a resource because it executes inside Chromium and integrates with the live Codex DOM. It is not a Node runtime dependency.

Security invariants:

- CDP HTTP and WebSocket endpoints must resolve to `127.0.0.1` or `localhost` on the selected port.
- Theme CSS and images have explicit size limits.
- Theme state uses atomic replacement.
- The official Codex bundle, package, signature, `app.asar`, authentication, and API configuration are never modified.
