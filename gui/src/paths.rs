use anyhow::{Context, Result};
use std::{env, fs, path::PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn state_root() -> Result<PathBuf> {
    let legacy_name = ["Codex", "Dream", "Skin", "Studio"].concat();
    let (root, legacy) = if cfg!(target_os = "windows") {
        let base = PathBuf::from(env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?);
        (base.join("CodeFace"), base.join(&legacy_name))
    } else {
        let base = PathBuf::from(env::var_os("HOME").context("HOME is not set")?)
            .join("Library/Application Support");
        (base.join("CodeFace"), base.join(&legacy_name))
    };
    if !root.exists() && legacy.exists() {
        fs::rename(&legacy, &root).or_else(|_| copy_legacy(&legacy, &root))?;
    }
    fs::create_dir_all(&root)?;
    migrate_theme_files(&root)?;
    Ok(root)
}

fn migrate_theme_files(root: &std::path::Path) -> std::io::Result<()> {
    let legacy_css = ["dream", "skin.css"].join("-");
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            migrate_theme_files(&entry.path())?;
        } else if entry.file_name() == legacy_css.as_str() {
            let target = entry.path().with_file_name("codeface.css");
            if !target.exists() {
                fs::rename(entry.path(), target)?;
            }
        }
    }
    Ok(())
}

fn copy_legacy(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_legacy(&entry.path(), &destination)?;
        } else {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

pub fn themes_root() -> Result<PathBuf> {
    let path = state_root()?.join("themes");
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn active_theme_root() -> Result<PathBuf> {
    let path = state_root()?.join("theme");
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn state_path() -> Result<PathBuf> {
    Ok(state_root()?.join("state.json"))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(state_root()?.join("injector.log"))
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(state_root()?.join("settings.json"))
}
