# CI and releases

## CI

`.github/workflows/ci.yml` runs on pushes to `main`, pull requests, and manual dispatch:

- `cargo fmt --check`
- macOS unit tests, Clippy, and a real `CodeFace.app` package build
- Native Windows unit tests and a real `CodeFace.exe` package build
- Cargo dependency caching
- Seven-day downloadable artifacts for both operating systems

The packaging steps deliberately use the same `cargo xtask package` command as
release builds. A pull request therefore cannot pass merely because the source
compiles; the platform-specific bundle layout, application icons, and macOS ad
hoc signing must also succeed.

## Build

`.github/workflows/build.yml` runs for version tags (`v*`) or when triggered manually:

- macOS runs `cargo xtask package` to build and archive `CodeFace.app`
- Windows runs `cargo xtask package` to build `CodeFace.exe` with the application icon embedded and archives it with `CodeFace.ico`
- Outputs for both platforms are uploaded as GitHub Actions artifacts
- Version tags automatically create a GitHub Release containing `CodeFace-macOS.zip` and `CodeFace-Windows.zip`

## Release checklist

1. Update the version in `gui/Cargo.toml`.
2. Update `CHANGELOG.md`.
3. Run local tests, Clippy, and checks for both platforms.
4. Run `cargo xtask package` and verify signing and startup behavior.
5. Create a `vX.Y.Z` tag.
6. Verify both artifacts and the generated GitHub Release.
7. Test theme application and restoration against real Codex installations on macOS and Windows.
