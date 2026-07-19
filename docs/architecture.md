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

## Runtime sequence

1. `ThemeStore` validates and atomically activates a theme.
2. `PlatformBackend` discovers the official Codex executable.
3. `CdpEngine` reuses a verified loopback endpoint or launches Codex with `--remote-debugging-address=127.0.0.1`.
4. Rust reads the embedded base CSS and renderer integration, appends theme CSS, and sends the payload over CDP WebSocket.
5. The same CodeFace executable starts in `--injector-daemon` mode and checks that the marker survives navigation.
6. If the marker disappears, the daemon reinjects the payload. It does not resend the full payload while the marker remains valid.

## Platform boundary

`PlatformBackend` contains only Codex discovery and lifecycle operations. Theme storage, image conversion, CDP, UI, settings, i18n, and packaging contracts are shared.

`HealthReport` is the shared post-apply contract between CLI and GUI. It samples each live Codex renderer through the loopback CDP client and records page identity, theme identity, critical-control visibility, text contrast, and suggestion stability before a switch is committed.

Theme history is stored under the CodeFace data root in `backups/<theme-id>/`; portable exports use `exports/<theme-id>.codex-theme`. Market discovery and package download remain separate operations so browsing never mutates the local library.

## Language selection

`i18n.rs` stores `system`, `english`, or `simplified-chinese`. `system` resolves through the native system locale at runtime. Rendering reads the effective locale each frame, so manual changes apply immediately.
