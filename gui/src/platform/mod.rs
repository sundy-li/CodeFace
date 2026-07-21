use anyhow::Result;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Debug)]
pub struct CodexInstall {
    pub executable: PathBuf,
}

pub trait PlatformBackend: Send + Sync {
    fn discover_codex(&self) -> Result<CodexInstall>;
    fn is_running(&self, install: &CodexInstall) -> bool;
    fn close_codex(&self, install: &CodexInstall) -> Result<()>;
    fn launch_codex(&self, install: &CodexInstall, cdp_port: Option<u16>) -> Result<()>;
    fn focus_codex(&self, install: &CodexInstall) -> Result<()>;
}

pub fn backend() -> &'static dyn PlatformBackend {
    #[cfg(target_os = "macos")]
    {
        &macos::MACOS
    }
    #[cfg(target_os = "windows")]
    {
        &windows::WINDOWS
    }
}
