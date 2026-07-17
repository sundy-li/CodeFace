use anyhow::{Context, Result, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const APP_NAME: &str = "CodeFace";
const BINARY_NAME: &str = "codeface";

fn workspace() -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask is not inside a workspace")?
        .to_path_buf())
}

fn run(command: &mut Command) -> Result<()> {
    let display = format!("{command:?}");
    if command
        .status()
        .with_context(|| format!("failed to start command: {display}"))?
        .success()
    {
        Ok(())
    } else {
        bail!("command failed: {display}")
    }
}

fn copy_file(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target).with_context(|| format!("failed to copy {}", source.display()))?;
    Ok(())
}

fn app_version(root: &Path) -> Result<String> {
    let manifest = fs::read_to_string(root.join("gui/Cargo.toml"))?;
    manifest
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("version = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .map(str::to_owned)
        .context("GUI Cargo.toml is missing version")
}

fn package_macos(root: &Path) -> Result<PathBuf> {
    run(Command::new("cargo").current_dir(root).args([
        "build",
        "--release",
        "--locked",
        "-p",
        BINARY_NAME,
    ]))?;
    let app = root.join("dist").join(format!("{APP_NAME}.app"));
    if app.exists() {
        fs::remove_dir_all(&app)?;
    }
    let contents = app.join("Contents");
    let executable = contents.join("MacOS/CodeFace");
    copy_file(&root.join("target/release").join(BINARY_NAME), &executable)?;
    copy_file(
        &root.join("resources/app-icon/CodeFace.icns"),
        &contents.join("Resources/CodeFace.icns"),
    )?;
    let version = app_version(root)?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDisplayName</key><string>{APP_NAME}</string>
<key>CFBundleExecutable</key><string>CodeFace</string>
<key>CFBundleIdentifier</key><string>com.codeface.app</string>
<key>CFBundleIconFile</key><string>CodeFace</string>
<key>CFBundleName</key><string>{APP_NAME}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{}</string>
<key>CFBundleVersion</key><string>{}</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#,
        version, version
    );
    fs::write(contents.join("Info.plist"), plist)?;
    run(Command::new("codesign").args([
        "--force",
        "--deep",
        "--sign",
        "-",
        app.to_str().context("app path is not valid UTF-8")?,
    ]))?;
    Ok(app)
}

fn package_windows(root: &Path) -> Result<PathBuf> {
    run(Command::new("cargo").current_dir(root).args([
        "build",
        "--release",
        "--locked",
        "-p",
        BINARY_NAME,
    ]))?;
    let output = root.join("dist/windows/CodeFace.exe");
    copy_file(&root.join("target/release/codeface.exe"), &output)?;
    copy_file(
        &root.join("resources/app-icon/CodeFace.ico"),
        &root.join("dist/windows/CodeFace.ico"),
    )?;
    Ok(output)
}

fn main() -> Result<()> {
    let root = workspace()?;
    let command = env::args().nth(1).unwrap_or_else(|| "package".into());
    if command != "package" {
        bail!("usage: cargo xtask package");
    }
    let artifact = if cfg!(target_os = "macos") {
        package_macos(&root)?
    } else if cfg!(target_os = "windows") {
        package_windows(&root)?
    } else {
        bail!("only macOS and Windows are currently supported")
    };
    println!("Created {}", artifact.display());
    Ok(())
}
