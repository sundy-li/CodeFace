use crate::{paths, platform};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, connect};
use url::Url;

const CSS: &str = include_str!("../../resources/assets/codeface.css");
const INJECTOR: &str = include_str!("../../resources/assets/codeface-inject.js");
const DEFAULT_PORT: u16 = 9341;

#[derive(Clone, Debug, Deserialize)]
struct Target {
    id: String,
    title: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeState {
    pub schema_version: u8,
    pub platform: String,
    pub port: u16,
    pub injector_pid: u32,
    pub codex_executable: String,
    pub theme_name: String,
    pub version: String,
}

fn targets(port: u16) -> Result<Vec<Target>> {
    let endpoint = format!("http://127.0.0.1:{port}/json/list");
    let values: Vec<Target> = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?
        .get(endpoint)
        .send()?
        .error_for_status()?
        .json()?;
    for target in &values {
        let url = Url::parse(&target.websocket_url)?;
        if url.scheme() != "ws"
            || url.host_str() != Some("127.0.0.1") && url.host_str() != Some("localhost")
            || url.port() != Some(port)
        {
            bail!("拒绝非本机 CDP WebSocket: {}", target.websocket_url);
        }
    }
    Ok(values)
}

pub fn endpoint_ready(port: u16) -> bool {
    targets(port).is_ok_and(|items| !items.is_empty())
}

fn select_port(preferred: u16) -> Result<u16> {
    for port in preferred..=preferred.saturating_add(100) {
        if TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).is_ok() {
            return Ok(port);
        }
    }
    bail!("未找到可用的本机 CDP 端口")
}

fn art_data_url(theme_root: &Path, theme: &Value) -> Result<String> {
    let image = theme
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let bytes = fs::read(theme_root.join(image)).context("读取主题背景图失败")?;
    if bytes.len() > 16 * 1024 * 1024 {
        bail!("背景图不能超过 16 MiB");
    }
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

fn payload(theme_root: &Path) -> Result<String> {
    let theme_text = fs::read_to_string(theme_root.join("theme.json"))?;
    let theme: Value = serde_json::from_str(&theme_text)?;
    let custom_css = fs::read_to_string(theme_root.join("codeface.css")).unwrap_or_default();
    if custom_css.len() > 256 * 1024 {
        bail!("codeface.css 不能超过 256 KiB");
    }
    Ok(INJECTOR
        .replace(
            "__CODEFACE_VERSION_JSON__",
            &serde_json::to_string(paths::VERSION)?,
        )
        .replace(
            "__CODEFACE_CSS_JSON__",
            &serde_json::to_string(&format!("{CSS}\n{custom_css}"))?,
        )
        .replace(
            "__CODEFACE_ART_JSON__",
            &serde_json::to_string(&art_data_url(theme_root, &theme)?)?,
        )
        .replace("__CODEFACE_THEME_JSON__", &theme_text))
}

fn evaluate(target: &Target, expression: &str) -> Result<Value> {
    let (mut socket, _) = connect(target.websocket_url.as_str())?;
    socket.send(Message::Text(
        json!({
          "id": 1,
          "method": "Runtime.evaluate",
          "params": { "expression": expression, "awaitPromise": true, "returnByValue": true }
        })
        .to_string()
        .into(),
    ))?;
    while let Ok(message) = socket.read() {
        if let Message::Text(text) = message {
            let response: Value = serde_json::from_str(&text)?;
            if response.get("id") == Some(&Value::from(1)) {
                if let Some(error) = response.get("error") {
                    bail!("CDP 执行失败: {error}");
                }
                return Ok(response
                    .pointer("/result/result/value")
                    .cloned()
                    .unwrap_or(Value::Null));
            }
        }
    }
    bail!("CDP 连接提前关闭")
}

pub fn inject_once(port: u16, theme_root: &Path) -> Result<usize> {
    let expression = payload(theme_root)?;
    let mut count = 0;
    for target in targets(port)?
        .into_iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        evaluate(&target, &expression).with_context(|| format!("注入目标 {} 失败", target.id))?;
        count += 1;
    }
    if count == 0 {
        bail!("未找到 Codex 渲染目标");
    }
    Ok(count)
}

pub fn verify(port: u16) -> Result<()> {
    let expression = "Boolean(document.documentElement.classList.contains('codeface') && document.getElementById('codeface-style'))";
    let passed = targets(port)?
        .iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
        .any(|target| evaluate(target, expression).is_ok_and(|value| value == Value::Bool(true)));
    if passed {
        Ok(())
    } else {
        bail!("实时页面未检测到 CodeFace 标记")
    }
}

fn write_state(state: &RuntimeState) -> Result<()> {
    let path = paths::state_path()?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(
        &temporary,
        format!("{}\n", serde_json::to_string_pretty(state)?),
    )?;
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn daemon(port: u16, theme_root: &Path) -> Result<()> {
    loop {
        if verify(port).is_err()
            && let Err(error) = inject_once(port, theme_root)
        {
            fs::write(paths::log_path()?, format!("{error:#}\n")).ok();
        }
        thread::sleep(Duration::from_secs(2));
    }
}

pub fn apply_active(theme_name: String, restart_existing: bool) -> Result<RuntimeState> {
    let backend = platform::backend();
    let install = backend.discover_codex()?;
    let previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok());
    let mut port = previous
        .as_ref()
        .map(|state| state.port)
        .unwrap_or(DEFAULT_PORT);
    if !endpoint_ready(port) {
        if backend.is_running(&install) {
            if !restart_existing {
                bail!("Codex 正在运行但没有 CDP 会话；请使用“重启并应用”");
            }
            backend.close_codex(&install)?;
        }
        port = select_port(port)?;
        backend.launch_codex(&install, Some(port))?;
        let deadline = Instant::now() + Duration::from_secs(45);
        while Instant::now() < deadline && !endpoint_ready(port) {
            thread::sleep(Duration::from_millis(350));
        }
        if !endpoint_ready(port) {
            bail!("Codex 未在 45 秒内开放本机 CDP 端口");
        }
    }
    inject_once(port, &paths::active_theme_root()?)?;
    verify(port)?;
    if let Some(state) = previous {
        stop_process(state.injector_pid);
    }
    let executable = std::env::current_exe()?;
    let child = Command::new(executable)
        .arg("--injector-daemon")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let state = RuntimeState {
        schema_version: 1,
        platform: std::env::consts::OS.into(),
        port,
        injector_pid: child.id(),
        codex_executable: install.executable.to_string_lossy().into_owned(),
        theme_name,
        version: paths::VERSION.into(),
    };
    write_state(&state)?;
    Ok(state)
}

pub fn close_codex() -> Result<()> {
    let backend = platform::backend();
    backend.close_codex(&backend.discover_codex()?)
}

pub fn restart_codex() -> Result<()> {
    let backend = platform::backend();
    let install = backend.discover_codex()?;
    backend.close_codex(&install)?;
    backend.launch_codex(&install, None)
}

fn stop_process(pid: u32) {
    let system = sysinfo::System::new_all();
    if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
        process.kill();
    }
}

pub fn remove_live_skin() -> Result<()> {
    if let Ok(text) = fs::read_to_string(paths::state_path()?)
        && let Ok(state) = serde_json::from_str::<RuntimeState>(&text)
    {
        let expression = "window.__CODEFACE_STATE__?.cleanup?.() ?? true";
        if endpoint_ready(state.port) {
            for target in targets(state.port)?
                .iter()
                .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
            {
                let _ = evaluate(target, expression);
            }
        }
        stop_process(state.injector_pid);
    }
    let _ = fs::remove_file(paths::state_path()?);
    Ok(())
}
