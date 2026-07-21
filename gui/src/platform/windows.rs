use super::{CodexInstall, PlatformBackend};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};
use sysinfo::{ProcessesToUpdate, Signal, System};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM},
    UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SW_RESTORE, SetForegroundWindow,
        ShowWindow,
    },
};
use windows_sys::core::BOOL;

pub struct WindowsBackend;
pub static WINDOWS: WindowsBackend = WindowsBackend;

fn find_executable(root: &Path, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if path.is_file()
            && matches!(
                path.file_name().and_then(|v| v.to_str()),
                Some("Codex.exe" | "ChatGPT.exe")
            )
        {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_executable(&path, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn candidates() -> Vec<PathBuf> {
    ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect()
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

struct WindowSearch {
    pid: u32,
    window: HWND,
}

unsafe extern "system" fn find_process_window(window: HWND, parameter: LPARAM) -> BOOL {
    let search = unsafe { &mut *(parameter as *mut WindowSearch) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid == search.pid && unsafe { IsWindowVisible(window) } != 0 {
        search.window = window;
        return 0;
    }
    1
}

impl PlatformBackend for WindowsBackend {
    fn discover_codex(&self) -> Result<CodexInstall> {
        for root in candidates() {
            if let Some(executable) = find_executable(&root, 5) {
                return Ok(CodexInstall { executable });
            }
        }
        bail!("could not find Codex.exe or ChatGPT.exe")
    }

    fn is_running(&self, install: &CodexInstall) -> bool {
        !matching_processes(install).is_empty()
    }

    fn close_codex(&self, install: &CodexInstall) -> Result<()> {
        let system = System::new_all();
        let pids = matching_processes(install);
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
        bail!("Codex did not exit within 12 seconds")
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
            let mut search = WindowSearch {
                pid: pid.as_u32(),
                window: std::ptr::null_mut(),
            };
            unsafe {
                EnumWindows(
                    Some(find_process_window),
                    &mut search as *mut WindowSearch as LPARAM,
                );
            }
            if search.window.is_null() {
                continue;
            }
            unsafe {
                ShowWindow(search.window, SW_RESTORE);
                if SetForegroundWindow(search.window) != 0 {
                    return Ok(());
                }
            }
        }
        bail!("failed to bring Codex to the foreground")
    }
}
