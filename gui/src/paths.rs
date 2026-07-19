use anyhow::{Context, Result};
use std::{env, fs, path::PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn state_root() -> Result<PathBuf> {
    let root = if cfg!(target_os = "windows") {
        let base = PathBuf::from(env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set")?);
        base.join("CodeFace")
    } else {
        let base = PathBuf::from(env::var_os("HOME").context("HOME is not set")?)
            .join("Library/Application Support");
        base.join("CodeFace")
    };
    fs::create_dir_all(&root)?;
    Ok(root)
}

pub fn themes_root() -> Result<PathBuf> {
    let path = state_root()?.join("themes");
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn backups_root() -> Result<PathBuf> {
    let path = state_root()?.join("backups");
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn exports_root() -> Result<PathBuf> {
    let path = state_root()?.join("exports");
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
