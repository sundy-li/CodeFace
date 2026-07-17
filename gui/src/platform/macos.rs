use super::{CodexInstall, PlatformBackend};
use anyhow::{Context, Result, bail};
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
        bail!("未找到 /Applications/Codex.app 或 ChatGPT.app")
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
        bail!("Codex 未在 12 秒内退出，请保存输入后手动关闭")
    }

    fn launch_codex(&self, install: &CodexInstall, cdp_port: Option<u16>) -> Result<()> {
        let mut command = Command::new(&install.executable);
        if let Some(port) = cdp_port {
            command
                .arg("--remote-debugging-address=127.0.0.1")
                .arg(format!("--remote-debugging-port={port}"));
        }
        command.spawn().context("启动 Codex 失败")?;
        Ok(())
    }
}
