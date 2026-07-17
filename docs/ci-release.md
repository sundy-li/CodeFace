# CI and releases

## CI

`.github/workflows/ci.yml` runs on pushes and pull requests:

- `cargo fmt --check`
- macOS unit tests and Clippy
- Native Windows `cargo check`
- Cargo dependency caching

## Build

`.github/workflows/build.yml` runs for version tags (`v*`) or when triggered manually:

- macOS runs `cargo xtask package` to build and archive `CodeFace.app`
- Windows runs `cargo xtask package` to build `CodeFace.exe` with the application icon embedded and packages `CodeFace.ico` for explicit shortcut configuration
- Outputs for both platforms are uploaded as GitHub Actions artifacts

## Release checklist

1. Update the version in `gui/Cargo.toml`.
2. Update `CHANGELOG.md`.
3. Run local tests, Clippy, and checks for both platforms.
4. Run `cargo xtask package` and verify signing and startup behavior.
5. Create a `vX.Y.Z` tag.
6. Verify both artifacts from the Build workflow.
7. Test theme application and restoration against real Codex installations on macOS and Windows.
