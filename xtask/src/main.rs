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
        .context("xtask 不在 workspace 中")?
        .to_path_buf())
}

fn run(command: &mut Command) -> Result<()> {
    let display = format!("{command:?}");
    if command.status()?.success() {
        Ok(())
    } else {
        bail!("命令失败: {display}")
    }
}

fn copy_file(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target).with_context(|| format!("复制 {} 失败", source.display()))?;
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
        .context("GUI Cargo.toml 缺少 version")
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
    let version = app_version(root)?;
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleDisplayName</key><string>{APP_NAME}</string>
<key>CFBundleExecutable</key><string>CodeFace</string>
<key>CFBundleIdentifier</key><string>com.codeface.app</string>
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
        app.to_str().context("App 路径不是 UTF-8")?,
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
    Ok(output)
}

fn main() -> Result<()> {
    let root = workspace()?;
    let command = env::args().nth(1).unwrap_or_else(|| "package".into());
    if command != "package" {
        bail!("用法: cargo xtask package");
    }
    let artifact = if cfg!(target_os = "macos") {
        package_macos(&root)?
    } else if cfg!(target_os = "windows") {
        package_windows(&root)?
    } else {
        bail!("当前仅支持 macOS 和 Windows")
    };
    println!("Created {}", artifact.display());
    Ok(())
}
