use crate::{paths, platform, theme};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::codecs::jpeg::JpegEncoder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
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
const SETTINGS_INJECTOR: &str = include_str!("../../resources/assets/codeface-settings.js");
const SETTINGS_CSS: &str = include_str!("../../resources/assets/codeface-settings.css");
const DEFAULT_PORT: u16 = 9341;
const THEME_POLL_INTERVAL: Duration = Duration::from_millis(750);
const THEME_SCAN_INTERVAL: Duration = Duration::from_secs(2);
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
    Ok(format!(
        "{}\n{}",
        theme_payload(theme_root)?,
        settings_payload()?
    ))
}

fn theme_payload(theme_root: &Path) -> Result<String> {
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

fn settings_payload() -> Result<String> {
    Ok(SETTINGS_INJECTOR
        .replace(
            "__CODEFACE_SETTINGS_CSS_JSON__",
            &serde_json::to_string(SETTINGS_CSS)?,
        )
        .replace(
            "__CODEFACE_SETTINGS_DATA_JSON__",
            &serde_json::to_string(&settings_catalog()?)?,
        ))
}

fn theme_thumbnail_data_url(path: &Path) -> Option<String> {
    let image = image::open(path).ok()?.thumbnail(320, 200).to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 78)
        .encode(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    (bytes.len() <= 256 * 1024)
        .then(|| format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)))
}

fn local_theme_preview_data_url(id: &str) -> Result<String> {
    if id.is_empty()
        || id.len() > 128
        || id.contains('/')
        || id.contains('\\')
        || id == "."
        || id == ".."
    {
        bail!("local preview theme ID is invalid");
    }
    let root = paths::themes_root()?.join(id);
    let manifest: Value = serde_json::from_str(&fs::read_to_string(root.join("theme.json"))?)?;
    let preview_name = manifest
        .get("preview")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let image_name = manifest
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let cached_market_preview = manifest
        .get("codexthemes")
        .is_some()
        .then(|| paths::market_previews_root().ok())
        .flatten()
        .map(|previews| previews.join(format!("{id}.png")))
        .filter(|path| path.is_file());
    let path = if !preview_name.is_empty() && root.join(preview_name).is_file() {
        root.join(preview_name)
    } else if let Some(path) = cached_market_preview {
        path
    } else {
        root.join(image_name)
    };
    let bytes = fs::read(&path)?;
    if bytes.len() > 8 * 1024 * 1024 {
        bail!("local theme preview cannot exceed 8 MiB");
    }
    let format =
        image::guess_format(&bytes).context("failed to detect local theme preview format")?;
    image::load_from_memory_with_format(&bytes, format)
        .context("failed to decode local theme preview")?;
    let mime = match format {
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        _ => "image/png",
    };
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn settings_catalog() -> Result<Value> {
    let state = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok());
    let locale = crate::i18n::Language::System.effective();
    let mut themes = vec![json!({
        "id": "__codeface-system-theme__",
        "name": crate::i18n::t(locale, "system_theme"),
        "description": crate::i18n::t(locale, "system_theme_badge"),
        "system": true,
        "image": "",
        "colors": ["#ffffff", "#f3f3f4", "#d8d8dc"]
    })];
    for entry in fs::read_dir(paths::themes_root()?)?.flatten() {
        let root = entry.path();
        let Some(id) = root.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(root.join("theme.json")) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let preview_name = manifest
            .get("preview")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let image_name = manifest
            .get("image")
            .and_then(Value::as_str)
            .unwrap_or("background.png");
        let cached_market_preview = manifest
            .get("codexthemes")
            .is_some()
            .then(|| paths::market_previews_root().ok())
            .flatten()
            .map(|previews| previews.join(format!("{id}.png")))
            .filter(|path| path.is_file());
        let preview_path = if !preview_name.is_empty() && root.join(preview_name).is_file() {
            root.join(preview_name)
        } else if let Some(path) = cached_market_preview {
            path
        } else {
            root.join(image_name)
        };
        let thumbnail = theme_thumbnail_data_url(&preview_path).unwrap_or_default();
        let colors = manifest.get("colors");
        let color = |name: &str, fallback: &str| {
            colors
                .and_then(|value| value.get(name))
                .and_then(Value::as_str)
                .unwrap_or(fallback)
                .to_owned()
        };
        themes.push(json!({
            "id": id,
            "name": manifest.get("name").and_then(Value::as_str).unwrap_or(id),
            "description": manifest.get("description").or_else(|| manifest.get("tagline")).and_then(Value::as_str).unwrap_or_else(|| crate::i18n::t(locale, "custom_theme")),
            "system": false,
            "market": manifest.get("codexthemes").is_some(),
            "preview": "",
            "thumbnail": thumbnail,
            "colors": [color("background", "#ffffff"), color("panel", "#ffffff"), color("accent", "#7c3aed")]
        }));
    }
    themes[1..].sort_by_key(|value| {
        value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase()
    });
    Ok(json!({
        "themes": themes,
        "appliedId": state.filter(|state| state.injection_enabled).map(|state| state.theme_id).unwrap_or_else(|| "__codeface-system-theme__".into()),
        "version": paths::VERSION
    }))
}

fn drain_settings_commands(port: u16) -> Result<Vec<Value>> {
    let expression = "(() => window.__CODEFACE_SETTINGS__?.drain?.() || [])()";
    let mut commands = Vec::new();
    for target in targets(port)?
        .iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        if let Ok(Value::Array(values)) = evaluate(target, expression) {
            commands.extend(values);
        }
    }
    Ok(commands)
}

fn send_settings_result(port: u16, request_id: &Value, result: Result<Value>) -> Result<()> {
    let payload = match result {
        Ok(value) => json!({ "ok": true, "value": value }),
        Err(error) => json!({ "ok": false, "error": format!("{error:#}") }),
    };
    let expression = format!(
        "window.__CODEFACE_SETTINGS__?.resolve?.({}, {})",
        serde_json::to_string(request_id)?,
        serde_json::to_string(&payload)?
    );
    for target in targets(port)?
        .iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        evaluate(target, &expression)?;
    }
    Ok(())
}

fn write_applied_state(id: &str, name: &str, enabled: bool) -> Result<()> {
    if let Ok(text) = fs::read_to_string(paths::state_path()?)
        && let Ok(mut state) = serde_json::from_str::<RuntimeState>(&text)
    {
        state.injection_enabled = enabled;
        state.theme_id = if enabled {
            id.to_owned()
        } else {
            String::new()
        };
        state.theme_name = name.to_owned();
        write_state(&state)?;
    }
    Ok(())
}

fn restore_embedded_native(port: u16) -> Result<()> {
    let expression = "(() => { window.__CODEFACE_STATE__?.cleanup?.(); window.__CODEFACE_SETTINGS__?.setApplied?.('__codeface-system-theme__'); return true; })()";
    for target in targets(port)?
        .iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        evaluate(target, expression)?;
    }
    write_applied_state("", "System", false)
}

fn apply_embedded_theme(port: u16, theme_root: &Path, id: &str) -> Result<Value> {
    if id == "__codeface-system-theme__" {
        restore_embedded_native(port)?;
        return settings_catalog();
    }
    let previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok())
        .filter(|state| state.injection_enabled && !state.theme_id.is_empty())
        .map(|state| state.theme_id);
    theme::activate(id)?;
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(theme_root.join("theme.json"))?)?;
    let name = manifest.get("name").and_then(Value::as_str).unwrap_or(id);
    write_applied_state(id, name, true)?;
    inject_theme_once(port, theme_root)?;
    let report = health_check(port, id)?;
    if !report.healthy {
        if let Some(previous_id) = previous {
            theme::activate(&previous_id)?;
            let previous_manifest: Value =
                serde_json::from_str(&fs::read_to_string(theme_root.join("theme.json"))?)?;
            let previous_name = previous_manifest
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&previous_id);
            write_applied_state(&previous_id, previous_name, true)?;
            inject_theme_once(port, theme_root)?;
        } else {
            restore_embedded_native(port)?;
        }
        bail!(
            "theme health check failed and was rolled back: {}",
            if report.issues.is_empty() {
                "unknown runtime failure".to_owned()
            } else {
                report.issues.join("; ")
            }
        );
    }
    settings_catalog()
}

fn setting_string<'a>(command: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    let value = command
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("settings command is missing {key}"))?;
    if value.len() > max {
        bail!("settings command {key} exceeds {max} bytes");
    }
    Ok(value)
}

fn handle_settings_command(port: u16, theme_root: &Path, command: &Value) -> Result<Value> {
    match command
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "refresh" => settings_catalog(),
        "apply" => apply_embedded_theme(port, theme_root, setting_string(command, "id", 128)?),
        "source" => {
            let id = setting_string(command, "id", 128)?;
            let (manifest, css, image) = theme::read_source(id)?;
            Ok(
                json!({ "id": id, "manifest": manifest, "css": css, "image": image.display().to_string() }),
            )
        }
        "save" => {
            let existing = command.get("existingId").and_then(Value::as_str);
            if existing.is_some_and(|id| id.len() > 128) {
                bail!("settings command existingId exceeds 128 bytes");
            }
            let uploaded_image = command
                .get("imageBase64")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(|encoded| -> Result<PathBuf> {
                    if encoded.len() > 22 * 1024 * 1024 {
                        bail!("theme image payload exceeds 22 MiB");
                    }
                    let bytes = STANDARD
                        .decode(encoded)
                        .context("theme image is not valid base64")?;
                    if bytes.len() > 16 * 1024 * 1024 {
                        bail!("theme image cannot exceed 16 MiB");
                    }
                    let path = paths::state_root()?
                        .join(format!(".embedded-image-{}.tmp", std::process::id()));
                    fs::write(&path, bytes)?;
                    Ok(path)
                })
                .transpose()?;
            let saved = theme::save(
                setting_string(command, "manifest", 256 * 1024)?,
                setting_string(command, "css", 256 * 1024)?,
                uploaded_image.as_deref(),
                existing,
            );
            if let Some(path) = uploaded_image {
                fs::remove_file(path).ok();
            }
            let id = saved?;
            let catalog = if command.get("apply").and_then(Value::as_bool) == Some(true) {
                apply_embedded_theme(port, theme_root, &id)?
            } else {
                settings_catalog()?
            };
            Ok(json!({ "catalog": catalog, "selectedId": id }))
        }
        "delete" => {
            let id = setting_string(command, "id", 128)?;
            let applied = fs::read_to_string(paths::state_path()?)
                .ok()
                .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok())
                .is_some_and(|state| state.injection_enabled && state.theme_id == id);
            if applied {
                restore_embedded_native(port)?;
            }
            theme::delete(id)?;
            settings_catalog()
        }
        "prompt" => Ok(json!({
            "text": theme::context_prompt(
                setting_string(command, "id", 128)?,
                matches!(crate::i18n::load().effective(), crate::i18n::Locale::SimplifiedChinese),
            )?
        })),
        "import-package" => {
            let encoded = setting_string(command, "base64", 42 * 1024 * 1024)?;
            let bytes = STANDARD
                .decode(encoded)
                .context("theme package is not valid base64")?;
            let id = theme::import_codextheme_package(&bytes)?;
            Ok(json!({ "catalog": settings_catalog()?, "selectedId": id }))
        }
        "import-directory" => {
            let files = command
                .get("files")
                .and_then(Value::as_array)
                .context("theme directory command is missing files")?;
            if files.is_empty() || files.len() > 32 {
                bail!("theme directory must contain between 1 and 32 files");
            }
            let staging =
                paths::state_root()?.join(format!(".embedded-import-{}", std::process::id()));
            if staging.exists() {
                fs::remove_dir_all(&staging)?;
            }
            fs::create_dir(&staging)?;
            let imported = (|| -> Result<String> {
                let mut total = 0usize;
                for file in files {
                    let name = setting_string(file, "name", 128)?;
                    if Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name) {
                        bail!("theme directory filenames must be plain relative names");
                    }
                    let encoded = setting_string(file, "base64", 42 * 1024 * 1024)?;
                    let bytes = STANDARD
                        .decode(encoded)
                        .context("theme directory file is not valid base64")?;
                    if bytes.len() > 16 * 1024 * 1024 {
                        bail!("a theme directory file cannot exceed 16 MiB");
                    }
                    total = total.saturating_add(bytes.len());
                    if total > 30 * 1024 * 1024 {
                        bail!("theme directory cannot exceed 30 MiB");
                    }
                    fs::write(staging.join(name), bytes)?;
                }
                theme::import_directory(&staging)
            })();
            fs::remove_dir_all(&staging).ok();
            let id = imported?;
            Ok(json!({ "catalog": settings_catalog()?, "selectedId": id }))
        }
        "export" => {
            let path = theme::export_theme(setting_string(command, "id", 128)?)?;
            Ok(json!({ "path": path.display().to_string() }))
        }
        "market-search" => Ok(serde_json::to_value(theme::search_codexthemes(
            setting_string(command, "query", 256)?,
        )?)?),
        "local-preview" => Ok(json!({
            "image": local_theme_preview_data_url(setting_string(command, "id", 128)?)?
        })),
        "market-preview" => {
            let market_theme: theme::MarketTheme = serde_json::from_value(
                command
                    .get("theme")
                    .cloned()
                    .context("market preview command is missing theme")?,
            )?;
            let path = theme::download_market_preview(&market_theme)?;
            let bytes = fs::read(path)?;
            if bytes.len() > 8 * 1024 * 1024 {
                bail!("market preview cannot exceed 8 MiB");
            }
            Ok(json!({
                "theme": market_theme,
                "image": format!("data:image/png;base64,{}", STANDARD.encode(bytes))
            }))
        }
        "market-install" => {
            let id = theme::install_from_codexthemes(setting_string(command, "source", 512)?)?;
            if command.get("apply").and_then(Value::as_bool) == Some(true) {
                apply_embedded_theme(port, theme_root, &id)
            } else {
                Ok(json!({ "catalog": settings_catalog()?, "selectedId": id }))
            }
        }
        "check-update" => Ok(json!({
            "available": theme::market_update_available(setting_string(command, "id", 128)?)?
        })),
        "rollback" => {
            let id = setting_string(command, "id", 128)?;
            theme::rollback_theme(id)?;
            let applied = fs::read_to_string(paths::state_path()?)
                .ok()
                .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok())
                .is_some_and(|state| state.injection_enabled && state.theme_id == id);
            if applied {
                apply_embedded_theme(port, theme_root, id)
            } else {
                settings_catalog()
            }
        }
        unknown => bail!("unsupported embedded settings command: {unknown}"),
    }
}

fn handle_settings_commands(port: u16, theme_root: &Path) -> Result<()> {
    for command in drain_settings_commands(port)? {
        let request_id = command.get("requestId").cloned().unwrap_or(Value::Null);
        let result = if serde_json::to_vec(&command)?.len() > 45 * 1024 * 1024 {
            Err(anyhow::anyhow!("embedded settings command exceeds 45 MiB"))
        } else {
            handle_settings_command(port, theme_root, &command)
        };
        send_settings_result(port, &request_id, result)?;
    }
    Ok(())
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

fn click_point(target: &Target, point: &Value) -> Result<()> {
    let x = point
        .get("x")
        .and_then(Value::as_f64)
        .context("missing click x")?;
    let y = point
        .get("y")
        .and_then(Value::as_f64)
        .context("missing click y")?;
    let (mut socket, _) = connect(target.websocket_url.as_str())?;
    for (id, event_type) in [(1, "mousePressed"), (2, "mouseReleased")] {
        socket.send(Message::Text(
            json!({
                "id": id,
                "method": "Input.dispatchMouseEvent",
                "params": { "type": event_type, "x": x, "y": y, "button": "left", "clickCount": 1 }
            })
            .to_string()
            .into(),
        ))?;
    }
    while let Ok(message) = socket.read() {
        if let Message::Text(text) = message {
            let response: Value = serde_json::from_str(&text)?;
            if response.get("id") == Some(&Value::from(2)) {
                return Ok(());
            }
        }
    }
    bail!("CDP connection closed before the click completed")
}

pub fn open_codeface_settings() -> Result<()> {
    let port = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok())
        .map(|state| state.port)
        .unwrap_or(DEFAULT_PORT);
    let target = targets(port)?
        .into_iter()
        .find(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
        .context("Codex renderer target not found")?;

    let settings_visible = evaluate(
        &target,
        "Boolean(document.querySelector('[data-settings-panel-slug=\"appearance\"]'))",
    )? == Value::Bool(true);
    if !settings_visible {
        let settings_expression = r#"(() => {
              const node=[...document.querySelectorAll('[role="menuitem"]')]
                .find(item => /(?:⌘|Ctrl)\s*,/.test(item.textContent));
              if (!node) return null; const r=node.getBoundingClientRect();
              return {x:r.x+r.width/2,y:r.y+r.height/2};
            })()"#;
        let mut settings = evaluate(&target, settings_expression)?;
        if settings.is_null() {
            let profile = evaluate(
                &target,
                r#"(() => {
                  const candidates = [...document.querySelectorAll('button[aria-haspopup="menu"]')]
                    .filter(node => { const r=node.getBoundingClientRect(); return r.width > 120 && r.height && r.x < 360 && r.y > innerHeight * .55; })
                    .sort((a,b) => b.getBoundingClientRect().width - a.getBoundingClientRect().width);
                  const node = candidates[0]; if (!node) return null;
                  const r=node.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2};
                })()"#,
            )?;
            click_point(&target, &profile).context("failed to open the Codex profile menu")?;
            thread::sleep(Duration::from_millis(180));
            settings = evaluate(&target, settings_expression)?;
        }
        click_point(&target, &settings).context("failed to open Codex Settings")?;
    }

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if evaluate(
            &target,
            "Boolean(document.querySelector('[data-settings-panel-slug=\"appearance\"]'))",
        )? == Value::Bool(true)
        {
            evaluate(&target, &settings_payload()?)?;
            let opened = evaluate(
                &target,
                "window.__CODEFACE_SETTINGS__?.open?.(); Boolean(document.querySelector('#codeface-settings-page[data-open=\"true\"]'))",
            )?;
            if opened == Value::Bool(true) {
                let backend = platform::backend();
                let install = backend.discover_codex()?;
                backend.focus_codex(&install)?;
                return Ok(());
            }
            bail!("CodeFace Settings bridge did not open its page")
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("Codex Settings did not open within 5 seconds")
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

fn inject_theme_once(port: u16, theme_root: &Path) -> Result<usize> {
    let expression = theme_payload(theme_root)?;
    let mut count = 0;
    for target in targets(port)?
        .into_iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        evaluate(&target, &expression)
            .with_context(|| format!("failed to inject theme into target {}", target.id))?;
        count += 1;
    }
    if count == 0 {
        bail!("no Codex renderer target found");
    }
    Ok(count)
}

fn inject_settings_once(port: u16) -> Result<usize> {
    let expression = settings_payload()?;
    let mut count = 0;
    for target in targets(port)?
        .into_iter()
        .filter(|target| target.url.starts_with("app://") || target.title.contains("Codex"))
    {
        evaluate(&target, &expression)
            .with_context(|| format!("failed to inject settings into target {}", target.id))?;
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
          const embeddedSettings = document.querySelector('#codeface-settings-page[data-open="true"]');
          const controls = (embeddedSettings ? [
            document.querySelector('div.app-shell-left-panel:has([data-settings-panel-slug])'),
            embeddedSettings
          ] : [
            document.querySelector('aside.app-shell-left-panel'),
            document.querySelector('div.app-shell-left-panel:has([data-settings-panel-slug])'),
            document.querySelector('main.main-surface'),
            document.querySelector('header'),
            document.querySelector('.composer-surface-chrome')
          ]).filter(Boolean);
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
          const contrastRoot = embeddedSettings || document;
          for (const node of [...contrastRoot.querySelectorAll('button, a, input, textarea, h1, h2, h3, label')].slice(0, 120)) {{
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
              const page = embeddedSettings ? 'codeface-settings' :
                (document.querySelector('main[data-codexthemes-page]')?.dataset.codexthemesPage ||
                (document.querySelector('[data-thread-find-target="conversation"]') ? 'conversation' : 'unknown'));
              const settingsHealthy = embeddedSettings && visible(embeddedSettings) && lowContrast <= 24;
              resolve({{
                page,
                themeId,
                criticalControls: controls.length,
                hiddenControls: hidden,
                lowContrastText: lowContrast,
                lowContrastSamples,
                suggestionRebuilds,
                healthy: themeId === expected && suggestionRebuilds === 0 &&
                  (embeddedSettings ? settingsHealthy :
                    controls.length >= 2 && hidden === 0 && lowContrast <= 4)
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
        if page.hidden_controls > 0 && page.page != "codeface-settings" {
            issues.push(format!(
                "{} has {} hidden critical controls",
                page.target_id, page.hidden_controls
            ));
        }
        let contrast_limit = if page.page == "codeface-settings" {
            24
        } else {
            4
        };
        if page.low_contrast_text > contrast_limit {
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
    if let Ok(text) = fs::read_to_string(paths::state_path()?)
        && let Ok(mut state) = serde_json::from_str::<RuntimeState>(&text)
    {
        state.port = port;
        state.injector_pid = std::process::id();
        write_state(&state)?;
    }
    let mut applied_fingerprint = theme_snapshot(theme_root)
        .ok()
        .map(|snapshot| snapshot.fingerprint);
    let mut pending_change: Option<(u64, Instant)> = None;
    let mut next_theme_scan = Instant::now();
    let mut next_verify = Instant::now();
    let mut last_error = String::new();

    loop {
        if !endpoint_ready(port) {
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        if let Err(error) = handle_settings_commands(port, theme_root) {
            write_daemon_error(&error, &mut last_error);
        }
        let theme_enabled = fs::read_to_string(paths::state_path()?)
            .ok()
            .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok())
            .is_none_or(|state| state.injection_enabled);
        let now = Instant::now();
        let source_root = selected_theme_root(theme_root).unwrap_or_else(|_| theme_root.to_owned());
        let snapshot = if now >= next_theme_scan {
            next_theme_scan = now + THEME_SCAN_INTERVAL;
            theme_enabled.then(|| theme_snapshot(&source_root))
        } else {
            None
        };
        match snapshot {
            Some(Ok(snapshot)) if Some(snapshot.fingerprint) != applied_fingerprint => {
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
                        .and_then(|()| inject_theme_once(port, theme_root).map(|_| ()));
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
            Some(Ok(_)) => pending_change = None,
            None => {}
            Some(Err(error)) => write_daemon_error(&error, &mut last_error),
        }

        if Instant::now() >= next_verify {
            let repair = if theme_enabled {
                verify(port)
                    .is_err()
                    .then(|| inject_once(port, theme_root).map(|_| ()))
            } else {
                let settings_missing = targets(port)?.iter().any(|target| {
                    (target.url.starts_with("app://") || target.title.contains("Codex"))
                        && evaluate(target, "Boolean(window.__CODEFACE_SETTINGS__)")
                            .is_ok_and(|value| value != Value::Bool(true))
                });
                settings_missing.then(|| inject_settings_once(port).map(|_| ()))
            };
            if let Some(Err(error)) = repair {
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
    let injector_pid = daemon_pid(port, previous.as_ref())?;
    let state = RuntimeState {
        schema_version: 3,
        platform: std::env::consts::OS.into(),
        port,
        injector_pid,
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
    let previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok());
    backend.close_codex(&install)?;
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
    resume_control_session(port, previous.as_ref())?;
    Ok(())
}

fn stop_process(pid: u32) {
    let system = sysinfo::System::new_all();
    if let Some(process) = system.process(sysinfo::Pid::from_u32(pid)) {
        process.kill();
    }
}

fn remove_theme_from_port(port: u16) -> Result<()> {
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

fn remove_skin_from_port(port: u16) -> Result<()> {
    let expression = r#"(() => {
      window.__CODEFACE_SETTINGS__?.cleanup?.();
      window.__CODEFACE_STATE__?.cleanup?.();
      document.documentElement.classList.remove('codeface');
      document.getElementById('codeface-style')?.remove();
      document.getElementById('codeface-settings-style')?.remove();
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

fn daemon_pid(port: u16, previous: Option<&RuntimeState>) -> Result<u32> {
    let current = std::process::id();
    if previous.is_some_and(|state| state.injector_pid == current) {
        return Ok(current);
    }
    if let Some(state) = previous.filter(|state| state.injector_pid != 0) {
        stop_process(state.injector_pid);
    }
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--injector-daemon")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    let child = command.spawn()?;
    let pid = child.id();
    thread::sleep(Duration::from_millis(250));
    let system = sysinfo::System::new_all();
    if system.process(sysinfo::Pid::from_u32(pid)).is_none() {
        bail!("injector daemon exited immediately after launch");
    }
    Ok(pid)
}

pub fn pause_control_for_update() -> Result<()> {
    let path = paths::state_path()?;
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(());
    };
    let mut state: RuntimeState = serde_json::from_str(&text)?;
    if state.injector_pid != 0 {
        stop_process(state.injector_pid);
        thread::sleep(Duration::from_millis(250));
        state.injector_pid = 0;
        write_state(&state)?;
    }
    Ok(())
}

pub fn resume_control_after_update() -> Result<()> {
    let path = paths::state_path()?;
    let Ok(text) = fs::read_to_string(&path) else {
        return Ok(());
    };
    let mut state: RuntimeState = serde_json::from_str(&text)?;
    state.injector_pid = daemon_pid(state.port, Some(&state))?;
    write_state(&state)
}

fn resume_control_session(port: u16, previous: Option<&RuntimeState>) -> Result<RuntimeState> {
    if !endpoint_ready(port) {
        bail!("Codex CDP endpoint is not available on 127.0.0.1:{port}");
    }
    let injection_enabled = previous.is_some_and(|state| state.injection_enabled);
    if injection_enabled {
        inject_once(port, &paths::active_theme_root()?)?;
        verify(port)?;
    } else {
        inject_settings_once(port)?;
    }
    let backend = platform::backend();
    let install = backend.discover_codex()?;
    let injector_pid = daemon_pid(port, previous)?;
    let state = RuntimeState {
        schema_version: 3,
        platform: std::env::consts::OS.into(),
        port,
        injector_pid,
        injection_enabled,
        codex_executable: install.executable.to_string_lossy().into_owned(),
        theme_name: previous
            .map(|state| state.theme_name.clone())
            .unwrap_or_else(|| "System".into()),
        theme_id: previous
            .map(|state| state.theme_id.clone())
            .unwrap_or_default(),
        version: paths::VERSION.into(),
    };
    write_state(&state)?;
    Ok(state)
}

pub fn start_control_bridge(port: u16) -> Result<RuntimeState> {
    let mut previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok());
    if let Some(state) = &mut previous {
        state.injection_enabled = false;
        state.theme_name = "System".into();
        state.theme_id.clear();
        write_state(state)?;
    }
    remove_theme_from_port(port)?;
    resume_control_session(port, previous.as_ref())
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
    let previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<RuntimeState>(&text).ok());
    let Some(mut state) = previous else {
        return Ok(());
    };
    if !endpoint_ready(state.port) {
        return remove_live_skin();
    }
    state.injection_enabled = false;
    state.theme_name = "System".into();
    state.theme_id.clear();
    write_state(&state)?;
    remove_theme_from_port(state.port)?;
    resume_control_session(state.port, Some(&state)).map(|_| ())
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
    fn embedded_settings_is_separate_from_theme_cleanup() {
        assert!(SETTINGS_INJECTOR.contains("const UI_VERSION = 9"));
        assert!(SETTINGS_INJECTOR.contains("codefaceSettingsEntry"));
        assert!(SETTINGS_INJECTOR.contains("__CODEFACE_SETTINGS__"));
        assert!(SETTINGS_INJECTOR.contains("data-settings-panel-slug"));
        assert!(!SETTINGS_CSS.contains("html.codeface"));
        assert!(!INJECTOR.contains("codeface-settings-style"));
    }

    #[test]
    fn full_restore_cleans_settings_but_native_mode_keeps_the_bridge() {
        assert!(SETTINGS_INJECTOR.contains("__CODEFACE_SETTINGS__"));
        assert!(SETTINGS_INJECTOR.contains("setApplied"));
        assert!(SETTINGS_CSS.contains("codeface-settings-page"));
        assert!(INJECTOR.contains("__CODEFACE_STATE__"));
    }

    #[test]
    fn embedded_settings_uses_a_bounded_command_queue() {
        assert!(SETTINGS_INJECTOR.contains("commands: []"));
        assert!(SETTINGS_INJECTOR.contains("drain: () => state.commands.splice(0)"));
        assert!(SETTINGS_INJECTOR.contains("state.commands.length >= 8"));
    }

    #[test]
    fn embedded_settings_exposes_native_manager_actions() {
        for marker in [
            "market-search",
            "market-preview",
            "market-install",
            "import-package",
            "import-directory",
            "Reference only",
            "Copy full prompt",
            "data-editor-save",
            "codeface-empty-state",
            "makeInteractive",
            "button.disabled=disabled",
            "Preview image failed to load",
            "codeface-overflow-menu",
            "More actions",
        ] {
            assert!(
                SETTINGS_INJECTOR.contains(marker),
                "embedded settings is missing {marker}"
            );
        }
        assert!(SETTINGS_INJECTOR.contains("codeface-icon-button"));
        assert!(SETTINGS_INJECTOR.contains("codeface-library-layout"));
        assert!(SETTINGS_INJECTOR.contains("codeface-market-layout"));
        assert!(SETTINGS_INJECTOR.contains("aria-label"));
        assert!(!SETTINGS_INJECTOR.contains("data-market-direct-install"));
        assert!(!SETTINGS_INJECTOR.contains("data-market-source"));
        assert!(SETTINGS_INJECTOR.contains(": selected.image"));
        assert!(SETTINGS_INJECTOR.contains("theme.thumbnail || theme.preview"));
        assert!(!SETTINGS_INJECTOR.contains("选择预览以加载大图"));
        assert!(!SETTINGS_INJECTOR.contains("data-codeface-tab=\"settings\""));
        assert!(!SETTINGS_INJECTOR.contains("data-save-settings"));
        assert!(SETTINGS_INJECTOR.contains("appearanceEntry.after(entry)"));
        assert!(SETTINGS_INJECTOR.contains("    open,"));
        assert!(SETTINGS_INJECTOR.contains("data-codeface-tab=\"local\""));
        assert!(SETTINGS_INJECTOR.contains("data-codeface-tab=\"market\""));
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
