use super::{CodexInstall, PlatformBackend};
use anyhow::{Context, Result, bail};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};
use sysinfo::{ProcessesToUpdate, Signal, System};

pub struct MacOsBackend;
pub static MACOS: MacOsBackend = MacOsBackend;

fn candidates() -> Vec<PathBuf> {
    let mut values = vec![
        PathBuf::from("/Applications/Codex.app"),
        PathBuf::from("/Applications/ChatGPT.app"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let applications = PathBuf::from(home).join("Applications");
        values.push(applications.join("Codex.app"));
        values.push(applications.join("ChatGPT.app"));
    }
    values
}

fn executable_for(bundle: &Path) -> Option<PathBuf> {
    ["Codex", "ChatGPT"]
        .into_iter()
        .map(|name| bundle.join("Contents/MacOS").join(name))
        .find(|path| path.is_file())
}

fn matching_processes(install: &CodexInstall) -> Vec<sysinfo::Pid> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let exe = process.exe()?;
            (exe == install.executable).then_some(*pid)
        })
        .collect()
}

impl PlatformBackend for MacOsBackend {
    fn discover_codex(&self) -> Result<CodexInstall> {
        for bundle in candidates() {
            if let Some(executable) = executable_for(&bundle) {
                return Ok(CodexInstall { executable });
            }
        }
        bail!("could not find /Applications/Codex.app or ChatGPT.app")
    }

    fn is_running(&self, install: &CodexInstall) -> bool {
        !matching_processes(install).is_empty()
    }

    fn close_codex(&self, install: &CodexInstall) -> Result<()> {
        let pids = matching_processes(install);
        if pids.is_empty() {
            return Ok(());
        }
        let system = System::new_all();
        for pid in &pids {
            if let Some(process) = system.process(*pid) {
                process.kill_with(Signal::Term);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(12);
        while Instant::now() < deadline {
            thread::sleep(Duration::from_millis(200));
            if matching_processes(install).is_empty() {
                return Ok(());
            }
        }
        bail!("Codex did not exit within 12 seconds; save your input and close it manually")
    }

    fn launch_codex(&self, install: &CodexInstall, cdp_port: Option<u16>) -> Result<()> {
        let mut command = Command::new(&install.executable);
        if let Some(port) = cdp_port {
            command
                .arg("--remote-debugging-address=127.0.0.1")
                .arg(format!("--remote-debugging-port={port}"));
        }
        command.spawn().context("failed to launch Codex")?;
        Ok(())
    }

    fn focus_codex(&self, install: &CodexInstall) -> Result<()> {
        for pid in matching_processes(install) {
            let Some(application) =
                NSRunningApplication::runningApplicationWithProcessIdentifier(pid.as_u32() as i32)
            else {
                continue;
            };
            application.unhide();
            if application.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows) {
                return Ok(());
            }
        }
        bail!("failed to bring Codex to the foreground")
    }
}
