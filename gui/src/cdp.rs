use crate::{paths, platform};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, connect};
use url::Url;

const CSS: &str = include_str!("../../resources/assets/codeface.css");
const INJECTOR: &str = include_str!("../../resources/assets/codeface-inject.js");
const DEFAULT_PORT: u16 = 9341;
const THEME_POLL_INTERVAL: Duration = Duration::from_millis(100);
const THEME_RELOAD_DEBOUNCE: Duration = Duration::from_millis(300);
const VERIFY_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct ThemeSnapshot {
    fingerprint: u64,
    asset_names: Vec<String>,
}

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
    pub injection_enabled: bool,
    pub codex_executable: String,
    pub theme_name: String,
    #[serde(default)]
    pub theme_id: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PageHealth {
    pub target_id: String,
    pub page: String,
    pub theme_id: String,
    pub critical_controls: u64,
    pub hidden_controls: u64,
    pub low_contrast_text: u64,
    pub low_contrast_samples: Vec<Value>,
    pub suggestion_rebuilds: u64,
    pub healthy: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct HealthReport {
    pub healthy: bool,
    pub expected_theme_id: String,
    pub pages: Vec<PageHealth>,
    pub issues: Vec<String>,
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
            bail!(
                "refusing non-loopback CDP WebSocket: {}",
                target.websocket_url
            );
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
    bail!("no available loopback CDP port found")
}

fn art_data_url(theme_root: &Path, theme: &Value) -> Result<String> {
    let image = theme
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let bytes =
        fs::read(theme_root.join(image)).context("failed to read theme background image")?;
    if bytes.len() > 16 * 1024 * 1024 {
        bail!("background image cannot exceed 16 MiB");
    }
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

fn optional_asset_data_url(theme_root: &Path, theme: &Value, field: &str) -> Result<String> {
    let Some(name) = theme.get(field).and_then(Value::as_str) else {
        return Ok(String::new());
    };
    let path = Path::new(name);
    if path.file_name().and_then(|value| value.to_str()) != Some(name) {
        bail!("theme {field} must be a file in the theme directory");
    }
    let bytes = fs::read(theme_root.join(name))
        .with_context(|| format!("failed to read theme {field} image"))?;
    if bytes.len() > 4 * 1024 * 1024 {
        bail!("theme {field} image cannot exceed 4 MiB");
    }
    image::load_from_memory(&bytes).with_context(|| format!("failed to decode theme {field}"))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

fn payload(theme_root: &Path) -> Result<String> {
    let theme_text = fs::read_to_string(theme_root.join("theme.json"))?;
    let theme: Value = serde_json::from_str(&theme_text)?;
    let name = theme
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if name.is_empty() {
        bail!("theme.json must contain a non-empty name");
    }
    let custom_css = fs::read_to_string(theme_root.join("codeface.css"))?;
    if custom_css.len() > 256 * 1024 {
        bail!("codeface.css cannot exceed 256 KiB");
    }
    let normalized_css = custom_css.to_ascii_lowercase();
    if ["@import", "@font-face", "url("]
        .iter()
        .any(|token| normalized_css.contains(token))
    {
        bail!("codeface.css cannot load external fonts, imports, or URL resources");
    }
    let css = if theme.get("codexthemes").is_some() {
        custom_css
    } else {
        format!("{CSS}\n{custom_css}")
    };
    Ok(INJECTOR
        .replace(
            "__CODEFACE_VERSION_JSON__",
            &serde_json::to_string(paths::VERSION)?,
        )
        .replace("__CODEFACE_CSS_JSON__", &serde_json::to_string(&css)?)
        .replace(
            "__CODEFACE_ART_JSON__",
            &serde_json::to_string(&art_data_url(theme_root, &theme)?)?,
        )
        .replace(
            "__CODEFACE_AVATAR_JSON__",
            &serde_json::to_string(&optional_asset_data_url(theme_root, &theme, "avatar")?)?,
        )
        .replace("__CODEFACE_THEME_JSON__", &theme_text))
}

fn theme_snapshot(theme_root: &Path) -> Result<ThemeSnapshot> {
    let theme_text = fs::read_to_string(theme_root.join("theme.json"))?;
    let theme: Value = serde_json::from_str(&theme_text)?;
    let image_name = theme
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let mut asset_names = vec![image_name.to_owned()];
    if let Some(avatar) = theme.get("avatar").and_then(Value::as_str) {
        asset_names.push(avatar.to_owned());
    }
    for name in &asset_names {
        let path = Path::new(name);
        if path.file_name().and_then(|value| value.to_str()) != Some(name.as_str()) {
            bail!("theme assets must be files in the theme directory");
        }
    }
    let css = fs::read_to_string(theme_root.join("codeface.css"))?;
    payload(theme_root)?;

    let mut hasher = DefaultHasher::new();
    theme_text.hash(&mut hasher);
    css.hash(&mut hasher);
    for name in &asset_names {
        name.hash(&mut hasher);
        fs::read(theme_root.join(name))?.hash(&mut hasher);
    }
    Ok(ThemeSnapshot {
        fingerprint: hasher.finish(),
        asset_names,
    })
}

fn selected_theme_root(active_root: &Path) -> Result<PathBuf> {
    let theme_text = fs::read_to_string(active_root.join("theme.json"))?;
    let theme: Value = serde_json::from_str(&theme_text)?;
    let id = theme.get("id").and_then(Value::as_str).unwrap_or_default();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        return Ok(active_root.to_owned());
    }
    let source = paths::themes_root()?.join(id);
    if source.join("theme.json").is_file() && source.join("codeface.css").is_file() {
        Ok(source)
    } else {
        Ok(active_root.to_owned())
    }
}

fn atomic_copy(source: &Path, target: &Path) -> Result<()> {
    let temporary = target.with_extension(format!("reload-{}.tmp", std::process::id()));
    fs::copy(source, &temporary)?;
    fs::rename(temporary, target)?;
    Ok(())
}

fn sync_active_theme(source: &Path, active: &Path, snapshot: &ThemeSnapshot) -> Result<()> {
    if source == active {
        return Ok(());
    }
    fs::create_dir_all(active)?;
    for name in &snapshot.asset_names {
        atomic_copy(&source.join(name), &active.join(name))?;
    }
    atomic_copy(&source.join("codeface.css"), &active.join("codeface.css"))?;
    // Commit the manifest last so readers never observe references to files
    // that have not finished syncing yet.
    atomic_copy(&source.join("theme.json"), &active.join("theme.json"))?;
    Ok(())
}

fn write_daemon_error(error: &anyhow::Error, previous: &mut String) {
    let message = format!("{error:#}");
    if *previous != message {
        fs::write(
            paths::log_path().unwrap_or_else(|_| PathBuf::from("injector.log")),
            format!("{message}\n"),
        )
        .ok();
        *previous = message;
    }
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
                    bail!("CDP execution failed: {error}");
                }
                if let Some(exception) = response.pointer("/result/exceptionDetails") {
                    bail!("CDP JavaScript exception: {exception}");
                }
                return Ok(response
                    .pointer("/result/result/value")
                    .cloned()
                    .unwrap_or(Value::Null));
            }
        }
    }
    bail!("CDP connection closed prematurely")
}

pub fn inject_once(port: u16, theme_root: &Path) -> Result<usize> {
    let expression = payload(theme_root)?;
    let mut count = 0;
    for target in targets(port)?
        .into_iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        evaluate(&target, &expression)
            .with_context(|| format!("failed to inject target {}", target.id))?;
        count += 1;
    }
    if count == 0 {
        bail!("no Codex renderer target found");
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
        bail!("CodeFace marker not detected on the live page")
    }
}

pub fn health_check(port: u16, expected_theme_id: &str) -> Result<HealthReport> {
    let expected = serde_json::to_string(expected_theme_id)?;
    let expression = format!(
        r#"new Promise((resolve) => {{
          const expected = {expected};
          const root = document.documentElement;
          const visible = (node) => {{
            if (!node) return false;
            const rect = node.getBoundingClientRect();
            const style = getComputedStyle(node);
            return rect.width >= 2 && rect.height >= 2 && style.display !== "none" &&
              style.visibility !== "hidden" && Number(style.opacity || 1) > 0.05;
          }};
          const controls = [
            document.querySelector('aside.app-shell-left-panel'),
            document.querySelector('main.main-surface'),
            document.querySelector('header'),
            document.querySelector('.composer-surface-chrome')
          ].filter(Boolean);
          const parse = (value) => {{
            const match = String(value).match(/rgba?\(\s*([\d.]+)[, ]+\s*([\d.]+)[, ]+\s*([\d.]+)/i);
            return match ? match.slice(1, 4).map(Number) : null;
          }};
          const luminance = (rgb) => {{
            const values = rgb.map((value) => {{
              const x = value / 255;
              return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
            }});
            return values[0] * 0.2126 + values[1] * 0.7152 + values[2] * 0.0722;
          }};
          const background = (node) => {{
            for (let current = node; current; current = current.parentElement) {{
              const style = getComputedStyle(current);
              const value = style.backgroundColor;
              if (value && value !== "transparent" && !value.endsWith(", 0)")) return parse(value);
              // A gradient or artwork is the visible background. Sampling an
              // opaque ancestor behind it would produce a false contrast result.
              if (style.backgroundImage && style.backgroundImage !== "none") return null;
            }}
            return null;
          }};
          let lowContrast = 0;
          const lowContrastSamples = [];
          for (const node of [...document.querySelectorAll('button, a, input, textarea, h1, h2, h3, label')].slice(0, 120)) {{
            if (!visible(node) || !(node.textContent?.trim() || node.value?.trim() || node.placeholder?.trim())) continue;
            const foreground = parse(getComputedStyle(node).color);
            const behind = background(node);
            if (!foreground || !behind) continue;
            const first = luminance(foreground), second = luminance(behind);
            const ratio = (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
            // This is a catastrophic-regression gate rather than a WCAG audit. The
            // sampled background can include translucent artwork, so only flag text
            // that is very likely unreadable.
            if (ratio < 1.8) {{
              lowContrast += 1;
              if (lowContrastSamples.length < 12) lowContrastSamples.push({{
                element: node.tagName.toLowerCase(),
                text: String(node.textContent || node.value || node.placeholder || '').trim().slice(0, 80),
                foreground: getComputedStyle(node).color,
                background: getComputedStyle(node).backgroundColor,
                sampledBackground: behind,
                ratio: Math.round(ratio * 100) / 100
              }});
            }}
          }}
          let suggestionRebuilds = 0;
          const suggestions = document.querySelector('.group\\/home-suggestions');
          const observer = new MutationObserver((records) => {{
            suggestionRebuilds += records.filter((record) => record.type === 'childList').length;
          }});
          setTimeout(() => {{
            if (suggestions) observer.observe(suggestions, {{ childList: true, subtree: true }});
            setTimeout(() => {{
              observer.disconnect();
              const themeId = root.dataset.codexthemesTheme ||
                (root.classList.contains('codeface') ? expected : '');
              const hidden = controls.filter((node) => !visible(node)).length;
              const page = document.querySelector('main[data-codexthemes-page]')?.dataset.codexthemesPage ||
                (document.querySelector('[data-thread-find-target="conversation"]') ? 'conversation' : 'unknown');
              resolve({{
                page,
                themeId,
                criticalControls: controls.length,
                hiddenControls: hidden,
                lowContrastText: lowContrast,
                lowContrastSamples,
                suggestionRebuilds,
                healthy: themeId === expected && controls.length >= 2 && hidden === 0 &&
                  lowContrast <= 4 && suggestionRebuilds === 0
              }});
            }}, 1800);
          }}, 750);
        }})"#
    );
    let mut pages = Vec::new();
    for target in targets(port)?
        .into_iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        let value = evaluate(&target, &expression)
            .with_context(|| format!("failed to inspect target {}", target.id))?;
        pages.push(PageHealth {
            target_id: target.id,
            page: value
                .get("page")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            theme_id: value
                .get("themeId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            critical_controls: value
                .get("criticalControls")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            hidden_controls: value
                .get("hiddenControls")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            low_contrast_text: value
                .get("lowContrastText")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            low_contrast_samples: value
                .get("lowContrastSamples")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            suggestion_rebuilds: value
                .get("suggestionRebuilds")
                .and_then(Value::as_u64)
                .unwrap_or_default(),
            healthy: value
                .get("healthy")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    let mut issues = Vec::new();
    for page in &pages {
        if page.theme_id != expected_theme_id {
            issues.push(format!(
                "{} did not report the expected theme",
                page.target_id
            ));
        }
        if page.critical_controls < 2 {
            issues.push(format!(
                "{} exposes only {} critical controls",
                page.target_id, page.critical_controls
            ));
        }
        if page.hidden_controls > 0 {
            issues.push(format!(
                "{} has {} hidden critical controls",
                page.target_id, page.hidden_controls
            ));
        }
        if page.low_contrast_text > 4 {
            issues.push(format!(
                "{} has {} low-contrast text samples",
                page.target_id, page.low_contrast_text
            ));
        }
        if page.suggestion_rebuilds > 0 {
            issues.push(format!(
                "{} rebuilt suggestions {} times",
                page.target_id, page.suggestion_rebuilds
            ));
        }
    }
    let healthy = !pages.is_empty() && pages.iter().all(|page| page.healthy);
    Ok(HealthReport {
        healthy,
        expected_theme_id: expected_theme_id.to_owned(),
        pages,
        issues,
    })
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
    let mut applied_fingerprint = theme_snapshot(theme_root)
        .ok()
        .map(|snapshot| snapshot.fingerprint);
    let mut pending_change: Option<(u64, Instant)> = None;
    let mut next_verify = Instant::now();
    let mut last_error = String::new();

    loop {
        let source_root = selected_theme_root(theme_root).unwrap_or_else(|_| theme_root.to_owned());
        match theme_snapshot(&source_root) {
            Ok(snapshot) if Some(snapshot.fingerprint) != applied_fingerprint => {
                let now = Instant::now();
                let stable_since = match pending_change {
                    Some((fingerprint, since)) if fingerprint == snapshot.fingerprint => since,
                    _ => {
                        pending_change = Some((snapshot.fingerprint, now));
                        now
                    }
                };
                if now.duration_since(stable_since) >= THEME_RELOAD_DEBOUNCE {
                    let reload = sync_active_theme(&source_root, theme_root, &snapshot)
                        .and_then(|()| inject_once(port, theme_root).map(|_| ()));
                    match reload {
                        Ok(()) => {
                            applied_fingerprint = Some(snapshot.fingerprint);
                            pending_change = None;
                            last_error.clear();
                            fs::remove_file(paths::log_path()?).ok();
                        }
                        Err(error) => {
                            write_daemon_error(&error, &mut last_error);
                            pending_change = Some((snapshot.fingerprint, now));
                        }
                    }
                }
            }
            Ok(_) => pending_change = None,
            Err(error) => write_daemon_error(&error, &mut last_error),
        }

        if Instant::now() >= next_verify {
            if verify(port).is_err()
                && let Err(error) = inject_once(port, theme_root)
            {
                write_daemon_error(&error, &mut last_error);
            }
            next_verify = Instant::now() + VERIFY_INTERVAL;
        }
        thread::sleep(THEME_POLL_INTERVAL);
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
                bail!("Codex is running without a CDP session; use Restart and Apply");
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
            bail!("Codex did not open a loopback CDP port within 45 seconds");
        }
    }
    let active_root = paths::active_theme_root()?;
    let active_manifest: Value =
        serde_json::from_str(&fs::read_to_string(active_root.join("theme.json"))?)?;
    let theme_id = active_manifest
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    inject_once(port, &active_root)?;
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
        schema_version: 3,
        platform: std::env::consts::OS.into(),
        port,
        injector_pid: child.id(),
        injection_enabled: true,
        codex_executable: install.executable.to_string_lossy().into_owned(),
        theme_name,
        theme_id,
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
    let previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok());
    let port = select_port(
        previous
            .as_ref()
            .map(|state| state.port)
            .unwrap_or(DEFAULT_PORT),
    )?;
    backend.launch_codex(&install, Some(port))?;
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline && !endpoint_ready(port) {
        thread::sleep(Duration::from_millis(350));
    }
    if !endpoint_ready(port) {
        bail!("Codex did not open a loopback CDP port within 45 seconds");
    }
    Ok(())
}

fn stop_process(pid: u32) {
    let system = sysinfo::System::new_all();
    if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
        process.kill();
    }
}

fn remove_skin_from_port(port: u16) -> Result<()> {
    let expression = r#"(() => {
      window.__CODEFACE_STATE__?.cleanup?.();
      document.documentElement.classList.remove('codeface');
      document.getElementById('codeface-style')?.remove();
      for (const node of document.querySelectorAll('[data-codeface]')) node.remove();
      return true;
    })()"#;
    for target in targets(port)?
        .iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        evaluate(target, expression)?;
    }
    Ok(())
}

pub fn remove_live_skin() -> Result<()> {
    let mut ports = vec![DEFAULT_PORT];
    let previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok());
    if let Some(state) = &previous {
        if !ports.contains(&state.port) {
            ports.push(state.port);
        }
        stop_process(state.injector_pid);
        thread::sleep(Duration::from_millis(150));
    }
    for port in ports {
        if endpoint_ready(port) {
            remove_skin_from_port(port)?;
        }
    }
    if let Some(mut state) = previous {
        state.schema_version = 3;
        state.injector_pid = 0;
        state.injection_enabled = false;
        write_state(&state)?;
    } else {
        let _ = fs::remove_file(paths::state_path()?);
    }
    Ok(())
}

pub fn restore_native() -> Result<()> {
    remove_live_skin()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_theme_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codeface-{label}-{nonce}"));
        fs::create_dir_all(&root).expect("create theme root");
        fs::write(
            root.join("theme.json"),
            r#"{"name":"Watch Test","image":"background.png"}"#,
        )
        .expect("write theme json");
        fs::write(root.join("codeface.css"), "html.codeface { color: red; }").expect("write css");
        RgbImage::from_pixel(1, 1, Rgb([255, 255, 255]))
            .save_with_format(root.join("background.png"), ImageFormat::Png)
            .expect("write image");
        root
    }

    #[test]
    fn theme_snapshot_changes_when_css_changes() {
        let root = test_theme_root("watch-change");
        let before = theme_snapshot(&root).expect("initial snapshot");
        fs::write(root.join("codeface.css"), "html.codeface { color: blue; }").expect("update css");
        let after = theme_snapshot(&root).expect("updated snapshot");
        assert_ne!(before.fingerprint, after.fingerprint);
        fs::remove_dir_all(root).expect("remove theme root");
    }

    #[test]
    fn runtime_state_accepts_schema_two_without_theme_id() {
        let state: RuntimeState = serde_json::from_str(
            r#"{
                "schema_version":2,
                "platform":"macos",
                "port":9341,
                "injector_pid":42,
                "injection_enabled":true,
                "codex_executable":"/Applications/Codex.app",
                "theme_name":"Legacy Theme",
                "version":"1.3.0"
            }"#,
        )
        .expect("deserialize schema-two state");
        assert!(state.theme_id.is_empty());
    }

    #[test]
    fn theme_snapshot_rejects_external_css_resources() {
        let root = test_theme_root("watch-invalid");
        fs::write(
            root.join("codeface.css"),
            "@import 'https://example.invalid/theme.css';",
        )
        .expect("update css");
        let error = theme_snapshot(&root).expect_err("forbidden CSS must fail");
        assert!(error.to_string().contains("cannot load external"));
        fs::remove_dir_all(root).expect("remove theme root");
    }

    #[test]
    fn codexthemes_payload_does_not_mix_in_codeface_layout_css() {
        let root = test_theme_root("codexthemes-payload");
        fs::write(
            root.join("theme.json"),
            r#"{"id":"market-test","name":"Market Test","image":"background.png","codexthemes":{"source":"https://codexthemes.ai/themes/market-test"}}"#,
        )
        .expect("write market manifest");
        fs::write(
            root.join("codeface.css"),
            r#":root[data-codexthemes-theme="market-test"] { color: tomato; }"#,
        )
        .expect("write market CSS");
        let source = payload(&root).expect("build market payload");
        assert!(source.contains("color: tomato"));
        assert!(!source.contains("radial-gradient(circle at 84% 4%"));
        assert!(source.contains("IS_CODEXTHEMES"));
        assert!(source.contains("data-codeface-codexthemes-surface"));
        assert!(source.contains("project-selector"));
        assert!(source.contains("suggestion"));
        fs::remove_dir_all(root).expect("remove theme root");
    }

    #[test]
    fn injector_detects_modern_composer_only_home_without_matching_conversations() {
        assert!(INJECTOR.contains(".composer-surface-chrome"));
        assert!(INJECTOR.contains("[data-thread-find-target=\"conversation\"]"));
        assert!(INJECTOR.contains("const home = classicHome || modernHome"));
    }

    #[test]
    fn project_and_composer_layout_has_a_shared_safety_gap() {
        for marker in [
            "codeface-project-selector",
            "codeface-project-bar",
            "codeface-project-section",
            "codeface-composer-stack",
        ] {
            assert!(INJECTOR.contains(&format!("syncMarker(\"{marker}\"")));
        }
        assert!(INJECTOR.contains("gap: 12px !important"));
        assert!(INJECTOR.contains("margin-bottom: 0 !important"));
        assert!(INJECTOR.contains("min-height: 68px !important"));
        assert!(CSS.contains(".codeface-home .codeface-project-bar"));

        for theme_css in [
            include_str!("../../resources/theme-packs/cyberpunk/codeface.css"),
            include_str!("../../resources/theme-packs/fzd/codeface.css"),
            include_str!("../../resources/theme-packs/lovely-girl/codeface.css"),
            include_str!("../../resources/theme-packs/messi/codeface.css"),
            include_str!("../../resources/theme-packs/qq2007/codeface.css"),
            include_str!("../../resources/theme-pack-template/codeface.css"),
        ] {
            assert!(theme_css.contains(".codeface-home .codeface-project-bar"));
            assert!(theme_css.contains(".codeface-project-selector > button"));
        }
    }
}
