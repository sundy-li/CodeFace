use crate::paths;
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Local;
use image::{DynamicImage, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const CODEXTHEMES_BASE_URL: &str = "https://codexthemes.ai";
const CODEXTHEMES_MAX_PACKAGE_SIZE: usize = 30 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarketTheme {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub image: String,
    pub url: String,
    pub kind: String,
    pub installable: bool,
    #[serde(rename = "downloadUrl", default)]
    pub download_url: String,
    #[serde(default)]
    pub verified: bool,
}

#[derive(Debug, Deserialize)]
struct MarketResponse {
    themes: Vec<MarketTheme>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BackupInfo {
    pub id: String,
    pub path: PathBuf,
    pub reason: String,
}

pub const DEFAULT_JSON: &str = include_str!("../../resources/theme-pack-template/theme.json");
pub const DEFAULT_CSS: &str = include_str!("../../resources/theme-pack-template/codeface.css");
const CODEFACE_SKILL_URL: &str =
    "https://raw.githubusercontent.com/sundy-li/CodeFace/refs/heads/main/skills/codeface/SKILL.md";

fn new_theme_json_for_root(root: &Path, date: &str) -> Result<String> {
    let mut version = 1_u32;
    let id = loop {
        let candidate = format!("theme-{date}-v{version}");
        if !root.join(&candidate).exists() {
            break candidate;
        }
        version += 1;
    };
    let mut value: Value = serde_json::from_str(DEFAULT_JSON)
        .context("the default theme template is not valid JSON")?;
    value["id"] = Value::String(id.clone());
    value["name"] = Value::String(id);
    Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
}

pub fn new_theme_json() -> Result<String> {
    let root = paths::themes_root()?;
    new_theme_json_for_root(&root, &Local::now().format("%Y%m%d").to_string())
}
struct BundledTheme {
    id: &'static str,
    json: &'static str,
    css: &'static str,
    background: &'static [u8],
    avatar: Option<&'static [u8]>,
}

include!(concat!(env!("OUT_DIR"), "/bundled_themes.rs"));

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("target file has no parent directory")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    fs::write(&temporary, data)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn copy_theme_directory(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    let mut total = 0_u64;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let size = entry.metadata()?.len();
        total = total.saturating_add(size);
        if total > CODEXTHEMES_MAX_PACKAGE_SIZE as u64 {
            bail!("theme snapshot cannot exceed 30 MiB");
        }
        fs::copy(entry.path(), target.join(entry.file_name()))?;
    }
    Ok(())
}

fn backup_theme_from_roots(
    id: &str,
    reason: &str,
    themes_root: &Path,
    backups_root: &Path,
) -> Result<BackupInfo> {
    codexthemes_id(id).or_else(|_| {
        if id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            Ok(id.to_owned())
        } else {
            bail!("invalid theme ID")
        }
    })?;
    let source = themes_root.join(id);
    if !source.join("theme.json").is_file() {
        bail!("theme does not exist: {id}");
    }
    let stamp = format!(
        "{}-{:09}",
        Local::now().format("%Y%m%d-%H%M%S"),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    );
    let reason_slug = safe_id(reason);
    let path = backups_root.join(id).join(format!("{stamp}-{reason_slug}"));
    copy_theme_directory(&source, &path)?;
    atomic_write(&path.join("backup-reason.txt"), reason.as_bytes())?;
    Ok(BackupInfo {
        id: id.to_owned(),
        path,
        reason: reason.to_owned(),
    })
}

pub fn backup_theme(id: &str, reason: &str) -> Result<BackupInfo> {
    backup_theme_from_roots(id, reason, &paths::themes_root()?, &paths::backups_root()?)
}

fn list_backups_from_root(id: &str, backups_root: &Path) -> Result<Vec<BackupInfo>> {
    let root = backups_root.join(id);
    let mut backups = Vec::new();
    for entry in fs::read_dir(root).into_iter().flatten().flatten() {
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let reason =
            fs::read_to_string(path.join("backup-reason.txt")).unwrap_or_else(|_| "backup".into());
        backups.push(BackupInfo {
            id: id.to_owned(),
            path,
            reason,
        });
    }
    backups.sort_by(|left, right| right.path.cmp(&left.path));
    Ok(backups)
}

pub fn list_backups(id: &str) -> Result<Vec<BackupInfo>> {
    list_backups_from_root(id, &paths::backups_root()?)
}

fn rollback_theme_from_roots(
    id: &str,
    themes_root: &Path,
    backups_root: &Path,
) -> Result<BackupInfo> {
    let backup = list_backups_from_root(id, backups_root)?
        .into_iter()
        .next()
        .context("no backup is available for this theme")?;
    if themes_root.join(id).exists() {
        backup_theme_from_roots(id, "before-rollback", themes_root, backups_root)?;
        fs::remove_dir_all(themes_root.join(id))?;
    }
    copy_theme_directory(&backup.path, &themes_root.join(id))?;
    let reason_file = themes_root.join(id).join("backup-reason.txt");
    if reason_file.exists() {
        fs::remove_file(reason_file)?;
    }
    Ok(backup)
}

pub fn rollback_theme(id: &str) -> Result<BackupInfo> {
    rollback_theme_from_roots(id, &paths::themes_root()?, &paths::backups_root()?)
}

fn safe_id(value: &str) -> String {
    let normalized: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("theme-{stamp}")
    } else {
        normalized.chars().take(64).collect()
    }
}

fn validate_json(source: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(source).context("theme.json is not valid JSON")?;
    let object = value
        .as_object()
        .context("the top level of theme.json must be an object")?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if name.is_empty() {
        bail!("theme.json must contain a non-empty name");
    }
    Ok(value)
}

fn write_background(source: Option<&Path>, target: &Path) -> Result<()> {
    let image = match source {
        Some(path) => image::open(path)
            .with_context(|| format!("failed to read background image {}", path.display()))?,
        None => DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255]))),
    };
    image.save_with_format(target, image::ImageFormat::Png)?;
    Ok(())
}

pub fn save(
    json: &str,
    css: &str,
    image: Option<&Path>,
    existing_id: Option<&str>,
) -> Result<String> {
    if css.len() > 256 * 1024 {
        bail!("codeface.css cannot exceed 256 KiB");
    }
    let normalized_css = css.to_ascii_lowercase();
    if ["@import", "@font-face", "url("]
        .iter()
        .any(|token| normalized_css.contains(token))
    {
        bail!("codeface.css cannot load external fonts, imports, or URL resources");
    }
    let mut value = validate_json(json)?;
    let name = value["name"].as_str().unwrap();
    let id = existing_id
        .map(str::to_owned)
        .unwrap_or_else(|| safe_id(value.get("id").and_then(Value::as_str).unwrap_or(name)));
    let root = paths::themes_root()?.join(&id);
    if root.join("theme.json").is_file() {
        backup_theme(&id, "before-edit")?;
    }
    fs::create_dir_all(&root)?;
    value["id"] = Value::String(id.clone());
    value["image"] = Value::String("background.png".into());
    atomic_write(
        &root.join("theme.json"),
        format!("{}\n", serde_json::to_string_pretty(&value)?).as_bytes(),
    )?;
    atomic_write(&root.join("codeface.css"), css.as_bytes())?;
    if image.is_some() || !root.join("background.png").is_file() {
        write_background(image, &root.join("background.png"))?;
    }
    Ok(id)
}

/// Installs shipped themes, replacing any existing theme with the same ID.
pub fn install_bundled_themes() -> Result<()> {
    install_bundled_themes_into(&paths::themes_root()?)
}

fn install_bundled_themes_into(themes_root: &Path) -> Result<()> {
    for theme in BUNDLED_THEMES {
        let root = themes_root.join(theme.id);
        fs::create_dir_all(&root)?;
        let mut value = validate_json(theme.json)?;
        value["id"] = Value::String(theme.id.into());
        value["image"] = Value::String("background.png".into());
        if theme.avatar.is_some() {
            value["avatar"] = Value::String("avatar.png".into());
        }
        atomic_write(
            &root.join("theme.json"),
            format!("{}\n", serde_json::to_string_pretty(&value)?).as_bytes(),
        )?;
        atomic_write(&root.join("codeface.css"), theme.css.as_bytes())?;
        atomic_write(&root.join("background.png"), theme.background)?;
        if let Some(avatar) = theme.avatar {
            atomic_write(&root.join("avatar.png"), avatar)?;
        } else if root.join("avatar.png").is_file() {
            fs::remove_file(root.join("avatar.png"))?;
        }
    }
    Ok(())
}

fn codexthemes_id(input: &str) -> Result<String> {
    let input = input.trim().trim_end_matches('/');
    let candidate = if input.starts_with("http://") || input.starts_with("https://") {
        let url = url::Url::parse(input).context("CodexThemes URL is invalid")?;
        if url.scheme() != "https" || url.host_str() != Some("codexthemes.ai") {
            bail!("only https://codexthemes.ai theme URLs are supported");
        }
        let segments: Vec<_> = url
            .path_segments()
            .context("CodexThemes URL does not contain a theme ID")?
            .filter(|segment| !segment.is_empty())
            .collect();
        match segments.as_slice() {
            ["themes", id] | ["zh", "themes", id] => (*id).to_owned(),
            _ => bail!(
                "CodexThemes URL must look like https://codexthemes.ai[/zh]/themes/<theme-id>"
            ),
        }
    } else {
        input.to_owned()
    };
    if candidate.is_empty()
        || candidate.len() > 64
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        bail!("CodexThemes theme ID must contain only lowercase letters, digits, and hyphens");
    }
    Ok(candidate)
}

fn rewrite_codexthemes_art_urls(css: &str, art_paths: &[&str]) -> Result<String> {
    let mut output = String::with_capacity(css.len());
    let mut remaining = css;
    while let Some(offset) = remaining.to_ascii_lowercase().find("url(") {
        output.push_str(&remaining[..offset]);
        let value_start = offset + 4;
        let tail = &remaining[value_start..];
        let close = tail
            .find(')')
            .context("theme CSS contains an unterminated url()")?;
        let raw = tail[..close].trim();
        let reference = raw
            .strip_prefix(['\'', '"'])
            .and_then(|value| value.strip_suffix(['\'', '"']))
            .unwrap_or(raw)
            .trim()
            .trim_start_matches("./");
        if !art_paths
            .iter()
            .any(|path| path.trim_start_matches("./") == reference)
        {
            bail!("CodexThemes package CSS contains an unsupported asset URL: {reference}");
        }
        output.push_str("var(--codeface-art)");
        remaining = &tail[close + 1..];
    }
    output.push_str(remaining);
    Ok(output)
}

fn install_codexthemes_package_into(
    package_bytes: &[u8],
    requested_id: &str,
    themes_root: &Path,
) -> Result<String> {
    if package_bytes.len() > CODEXTHEMES_MAX_PACKAGE_SIZE {
        bail!("CodexThemes package cannot exceed 30 MiB");
    }
    let package: Value = serde_json::from_slice(package_bytes)
        .context("CodexThemes package is not valid UTF-8 JSON")?;
    if package.get("format").and_then(Value::as_str) != Some("codex-theme")
        || package.get("schemaVersion").and_then(Value::as_u64) != Some(1)
    {
        bail!("unsupported CodexThemes package format or schema version");
    }
    let manifest = package
        .get("manifest")
        .and_then(Value::as_object)
        .context("CodexThemes package is missing its manifest")?;
    let id = manifest
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if id != requested_id {
        bail!("downloaded CodexThemes package ID does not match the requested theme");
    }
    codexthemes_id(id)?;
    let name = manifest
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if name.is_empty() {
        bail!("CodexThemes manifest must contain a non-empty displayName");
    }
    let css = package
        .get("css")
        .and_then(Value::as_str)
        .context("CodexThemes package is missing its CSS")?;
    if css.len() > 256 * 1024 {
        bail!("CodexThemes theme CSS cannot exceed 256 KiB");
    }
    let art = package
        .get("art")
        .and_then(Value::as_object)
        .context("CodexThemes package is missing its artwork")?;
    let art_name = art
        .get("filename")
        .and_then(Value::as_str)
        .context("CodexThemes artwork is missing its filename")?;
    if Path::new(art_name)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(art_name)
    {
        bail!("CodexThemes artwork filename must be a plain relative filename");
    }
    let manifest_art = manifest
        .get("art")
        .and_then(Value::as_str)
        .unwrap_or(art_name);
    let css = rewrite_codexthemes_art_urls(css, &[manifest_art, art_name])?;
    let art_bytes = STANDARD
        .decode(
            art.get("base64")
                .and_then(Value::as_str)
                .context("CodexThemes artwork is missing its base64 data")?,
        )
        .context("CodexThemes artwork is not valid base64")?;
    if art_bytes.len() > 16 * 1024 * 1024 {
        bail!("CodexThemes artwork cannot exceed 16 MiB");
    }
    let artwork =
        image::load_from_memory(&art_bytes).context("failed to decode CodexThemes artwork")?;

    let target = themes_root.join(id);
    if target.exists() {
        let installed: Value = serde_json::from_str(
            &fs::read_to_string(target.join("theme.json"))
                .context("failed to inspect the existing theme before update")?,
        )?;
        if installed
            .get("codexthemes")
            .and_then(|value| value.get("source"))
            .and_then(Value::as_str)
            != Some(format!("{CODEXTHEMES_BASE_URL}/themes/{id}").as_str())
        {
            bail!("theme {id} already exists and is not a CodexThemes installation");
        }
        if themes_root == paths::themes_root()? {
            backup_theme_from_roots(
                id,
                "before-market-update",
                themes_root,
                &paths::backups_root()?,
            )?;
        }
    }
    fs::create_dir_all(themes_root)?;
    let staging = themes_root.join(format!(".{id}.codexthemes-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir(&staging)?;

    let palette = manifest.get("palette");
    let color = |key: &str, fallback: &str| {
        palette
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let focal_point = manifest
        .get("design")
        .and_then(|value| value.get("artFocalPoint"))
        .and_then(Value::as_str)
        .unwrap_or("center center");
    let codeface_manifest = serde_json::json!({
        "id": id,
        "name": name,
        "description": manifest.get("description").and_then(Value::as_str).unwrap_or("CodexThemes theme"),
        "image": "background.png",
        "colors": {
            "background": color("canvas", "#111111"),
            "panel": color("surface", "#191919"),
            "panelAlt": color("raised", "#242424"),
            "accent": color("accent", "#7C3AED"),
            "accentAlt": color("focus", "#9B87FF"),
            "text": color("text", "#F5F5F5"),
            "muted": color("muted", "#A0A0A0"),
            "line": color("border", "#383838")
        },
        "layout": { "backgroundPosition": focal_point },
        "codexthemes": {
            "source": format!("{CODEXTHEMES_BASE_URL}/themes/{id}"),
            "version": manifest.get("version").cloned().unwrap_or(Value::Null),
            "backgroundScope": manifest.get("design").and_then(|value| value.get("backgroundScope")).cloned().unwrap_or(Value::String("home".into()))
        }
    });

    let result = (|| -> Result<()> {
        atomic_write(
            &staging.join("theme.json"),
            format!("{}\n", serde_json::to_string_pretty(&codeface_manifest)?).as_bytes(),
        )?;
        atomic_write(&staging.join("codeface.css"), css.as_bytes())?;
        artwork.save_with_format(staging.join("background.png"), image::ImageFormat::Png)?;
        let backup = themes_root.join(format!(".{id}.codexthemes-backup-{}", std::process::id()));
        if backup.exists() {
            fs::remove_dir_all(&backup)?;
        }
        if target.exists() {
            fs::rename(&target, &backup)?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(error.into());
        }
        if backup.exists() {
            fs::remove_dir_all(backup)?;
        }
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(id.to_owned())
}

pub fn search_codexthemes(query: &str) -> Result<Vec<MarketTheme>> {
    let response: MarketResponse = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?
        .get(format!("{CODEXTHEMES_BASE_URL}/api/themes"))
        .query(&[("q", query.trim()), ("limit", "20")])
        .send()
        .context("failed to search CodexThemes")?
        .error_for_status()
        .context("CodexThemes search failed")?
        .json()
        .context("CodexThemes search response is invalid")?;
    Ok(response.themes)
}

fn download_codexthemes_package(id: &str) -> Result<Vec<u8>> {
    let endpoint = format!("{CODEXTHEMES_BASE_URL}/api/themes/{id}/download");
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .get(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .context("failed to download theme from CodexThemes")?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::PAYMENT_REQUIRED
    {
        bail!(
            "CodexThemes anonymous download quota is exhausted; configure an API key with the official installer or try again later"
        );
    }
    let response = response
        .error_for_status()
        .with_context(|| format!("CodexThemes download failed with HTTP {status}"))?;
    let final_url = response.url();
    let final_host = final_url.host_str().unwrap_or_default();
    if final_url.scheme() != "https"
        || !(final_host == "codexthemes.ai" || final_host.ends_with(".codexthemes.ai"))
    {
        bail!("CodexThemes download redirected outside the trusted HTTPS domain");
    }
    if response
        .content_length()
        .is_some_and(|length| length > CODEXTHEMES_MAX_PACKAGE_SIZE as u64)
    {
        bail!("CodexThemes package cannot exceed 30 MiB");
    }
    let mut bytes = Vec::new();
    response
        .take(CODEXTHEMES_MAX_PACKAGE_SIZE as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > CODEXTHEMES_MAX_PACKAGE_SIZE {
        bail!("CodexThemes package cannot exceed 30 MiB");
    }
    Ok(bytes)
}

fn package_version(bytes: &[u8]) -> Result<String> {
    let package: Value = serde_json::from_slice(bytes).context("CodexThemes package is invalid")?;
    Ok(package
        .pointer("/manifest/version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned())
}

pub fn market_version(id: &str) -> Result<String> {
    let id = codexthemes_id(id)?;
    package_version(&download_codexthemes_package(&id)?)
}

pub fn installed_market_version(id: &str) -> Result<Option<String>> {
    let manifest: Value = serde_json::from_str(&fs::read_to_string(
        paths::themes_root()?.join(id).join("theme.json"),
    )?)?;
    Ok(manifest
        .pointer("/codexthemes/version")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

pub fn market_update_available(id: &str) -> Result<bool> {
    let installed = installed_market_version(id)?.unwrap_or_default();
    Ok(!installed.is_empty() && market_version(id)? != installed)
}

pub fn install_from_codexthemes(input: &str) -> Result<String> {
    let id = codexthemes_id(input)?;
    let bytes = download_codexthemes_package(&id)?;
    install_codexthemes_package_into(&bytes, &id, &paths::themes_root()?)
}

fn export_theme_from_roots(id: &str, themes_root: &Path, exports_root: &Path) -> Result<PathBuf> {
    let root = themes_root.join(id);
    let manifest: Value = serde_json::from_str(&fs::read_to_string(root.join("theme.json"))?)?;
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .context("theme name is missing")?;
    let image_name = manifest
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let image_path = root.join(image_name);
    let image = image::open(&image_path).context("failed to decode theme artwork")?;
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    if png.len() > 16 * 1024 * 1024 {
        bail!("exported artwork cannot exceed 16 MiB");
    }
    let css = fs::read_to_string(root.join("codeface.css"))?
        .replace("var(--codeface-art)", "url(\"./assets/artwork.png\")");
    let colors = manifest.get("colors");
    let color = |key: &str, fallback: &str| {
        colors
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let package = serde_json::json!({
        "format": "codex-theme",
        "schemaVersion": 1,
        "manifest": {
            "schemaVersion": 1,
            "id": id,
            "displayName": name,
            "description": manifest.get("description").cloned().unwrap_or(Value::String("Exported from CodeFace".into())),
            "version": manifest.pointer("/codexthemes/version").cloned().unwrap_or(Value::String("1.0.0".into())),
            "mode": "dark",
            "css": "theme.css",
            "art": "assets/artwork.png",
            "design": {
                "backgroundScope": manifest.pointer("/codexthemes/backgroundScope").cloned().unwrap_or(Value::String("home".into())),
                "artFocalPoint": manifest.pointer("/layout/backgroundPosition").cloned().unwrap_or(Value::String("center center".into()))
            },
            "palette": {
                "canvas": color("background", "#111111"),
                "surface": color("panel", "#191919"),
                "raised": color("panelAlt", "#242424"),
                "text": color("text", "#F5F5F5"),
                "muted": color("muted", "#A0A0A0"),
                "accent": color("accent", "#7C3AED"),
                "border": color("line", "#383838")
            },
            "platforms": ["macos", "windows"]
        },
        "css": css,
        "readme": format!("# {name}\n\nExported from CodeFace.\n"),
        "art": {
            "filename": "artwork.png",
            "mimeType": "image/png",
            "base64": STANDARD.encode(png)
        }
    });
    fs::create_dir_all(exports_root)?;
    let path = exports_root.join(format!("{id}.codex-theme"));
    let bytes = serde_json::to_vec_pretty(&package)?;
    if bytes.len() > CODEXTHEMES_MAX_PACKAGE_SIZE {
        bail!("export package cannot exceed 30 MiB");
    }
    atomic_write(&path, &bytes)?;
    Ok(path)
}

pub fn export_theme(id: &str) -> Result<PathBuf> {
    export_theme_from_roots(id, &paths::themes_root()?, &paths::exports_root()?)
}

pub fn import_directory(source: &Path) -> Result<String> {
    let json = fs::read_to_string(source.join("theme.json"))
        .context("theme pack is missing theme.json")?;
    let css = fs::read_to_string(source.join("codeface.css"))
        .context("theme pack is missing codeface.css")?;
    let value = validate_json(&json)?;
    let image_name = value
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let image = source.join(image_name);
    if !image.is_file() {
        bail!("theme pack background image does not exist: {image_name}");
    }
    let id = save(&json, &css, Some(&image), None)?;
    if let Some(avatar_name) = value.get("avatar").and_then(Value::as_str) {
        let avatar = source.join(avatar_name);
        if !avatar.is_file() {
            bail!("theme pack avatar image does not exist: {avatar_name}");
        }
        fs::copy(avatar, paths::themes_root()?.join(&id).join("avatar.png"))?;
        let manifest_path = paths::themes_root()?.join(&id).join("theme.json");
        let mut manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
        manifest["avatar"] = Value::String("avatar.png".into());
        atomic_write(
            &manifest_path,
            format!("{}\n", serde_json::to_string_pretty(&manifest)?).as_bytes(),
        )?;
    }
    Ok(id)
}

pub fn activate(id: &str) -> Result<PathBuf> {
    if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        bail!("invalid theme ID");
    }
    let source = paths::themes_root()?.join(id);
    if !source.join("theme.json").is_file() {
        bail!("theme does not exist: {id}");
    }
    let target = paths::active_theme_root()?;
    for entry in fs::read_dir(&target)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    let manifest: Value = serde_json::from_str(&fs::read_to_string(source.join("theme.json"))?)?;
    let mut names = vec![
        "theme.json".to_owned(),
        "codeface.css".to_owned(),
        "background.png".to_owned(),
    ];
    if let Some(avatar) = manifest.get("avatar").and_then(Value::as_str) {
        names.push(avatar.to_owned());
    }
    for name in names {
        fs::copy(source.join(&name), target.join(&name))
            .with_context(|| format!("failed to copy theme file {name}"))?;
    }
    Ok(target)
}

pub fn read_source(id: &str) -> Result<(String, String, PathBuf)> {
    let root = paths::themes_root()?.join(id);
    let json = fs::read_to_string(root.join("theme.json"))?;
    let value: Value = serde_json::from_str(&json)?;
    let image = root.join(
        value
            .get("image")
            .and_then(Value::as_str)
            .unwrap_or("background.png"),
    );
    Ok((json, fs::read_to_string(root.join("codeface.css"))?, image))
}

fn delete_from_root(root: &Path, id: &str) -> Result<()> {
    if id == "__codeface-system-theme__" {
        bail!("The system theme cannot be deleted");
    }
    if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        bail!("Invalid theme ID");
    }
    let theme_root = root.join(id);
    if !theme_root.is_dir() {
        bail!("Theme does not exist: {id}");
    }
    fs::remove_dir_all(theme_root)?;
    Ok(())
}

pub fn delete(id: &str) -> Result<()> {
    if id == "__codeface-system-theme__" {
        bail!("The system theme cannot be deleted");
    }
    backup_theme(id, "before-delete")?;
    delete_from_root(&paths::themes_root()?, id)
}

pub fn context_prompt(id: &str, chinese: bool) -> Result<String> {
    let root = paths::themes_root()?.join(id);
    context_prompt_for_root(&root, chinese)
}

fn context_prompt_for_root(root: &Path, chinese: bool) -> Result<String> {
    fs::metadata(root)?;
    fs::metadata(root.join("theme.json"))?;
    fs::metadata(root.join("codeface.css"))?;
    let prompt = if chinese {
        format!(
            "请先读取并遵循 CodeFace Skill，然后直接优化并验证指定主题。\n\nSkill 文档：\n{CODEFACE_SKILL_URL}\n\n主题目录：\n{root}\n",
            root = root.display(),
        )
    } else {
        format!(
            "Read and follow the CodeFace Skill first, then directly refine and verify the specified theme.\n\nSkill document:\n{CODEFACE_SKILL_URL}\n\nTheme directory:\n{root}\n",
            root = root.display(),
        )
    };
    Ok(prompt)
}

pub async fn choose_image(title: String, filter_name: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .add_filter(&filter_name, &["png", "jpg", "jpeg", "webp"])
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf())
}

pub async fn choose_pack(title: String) -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title(title)
        .pick_folder()
        .await
        .map(|folder| folder.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_theme_and_normalizes_id() {
        let value = validate_json(r#"{"name":"Sample Theme"}"#).expect("valid theme");
        assert_eq!(value["name"], "Sample Theme");
        assert_eq!(safe_id("CodeFace 01"), "codeface-01");
        assert!(validate_json("{}").is_err());
    }

    #[test]
    fn parses_codexthemes_ids_and_urls() {
        assert_eq!(
            codexthemes_id("https://codexthemes.ai/themes/portal-panic").expect("theme URL"),
            "portal-panic"
        );
        assert_eq!(
            codexthemes_id("https://codexthemes.ai/zh/themes/shenron-starwish")
                .expect("localized theme URL"),
            "shenron-starwish"
        );
        assert_eq!(
            codexthemes_id("portal-panic").expect("theme ID"),
            "portal-panic"
        );
        assert!(codexthemes_id("https://example.com/themes/portal-panic").is_err());
        assert!(codexthemes_id("Portal Panic").is_err());
    }

    #[test]
    fn market_results_deserialize_and_preserve_installability() {
        let response: MarketResponse = serde_json::from_value(serde_json::json!({
            "themes": [{
                "id": "coast",
                "name": "Coast",
                "description": "Sea glass",
                "author": "Designer",
                "mode": "dark",
                "image": "https://cdn.codexthemes.ai/coast.png",
                "url": "https://codexthemes.ai/themes/coast",
                "kind": "theme",
                "installable": true,
                "downloadUrl": "https://codexthemes.ai/api/themes/coast/download",
                "verified": true
            }]
        }))
        .expect("parse market response");
        assert_eq!(response.themes.len(), 1);
        assert!(response.themes[0].installable);
        assert_eq!(
            response.themes[0].download_url,
            "https://codexthemes.ai/api/themes/coast/download"
        );
    }

    #[test]
    fn backups_restore_previous_theme_files() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codeface-history-{nonce}"));
        let themes = root.join("themes");
        let backups = root.join("backups");
        let theme_root = themes.join("history-test");
        fs::create_dir_all(&theme_root).expect("create theme");
        fs::write(theme_root.join("theme.json"), r#"{"name":"Before"}"#).expect("write manifest");
        fs::write(theme_root.join("codeface.css"), "before").expect("write CSS");
        backup_theme_from_roots("history-test", "before-edit", &themes, &backups)
            .expect("create backup");
        fs::write(theme_root.join("codeface.css"), "after").expect("update CSS");
        rollback_theme_from_roots("history-test", &themes, &backups).expect("restore backup");
        assert_eq!(
            fs::read_to_string(theme_root.join("codeface.css")).expect("read restored CSS"),
            "before"
        );
        assert_eq!(
            list_backups_from_root("history-test", &backups)
                .expect("list backups")
                .len(),
            2,
            "rollback should preserve the replaced version too"
        );
        fs::remove_dir_all(root).expect("remove history root");
    }

    #[test]
    fn export_package_contains_manifest_css_and_png_artwork() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("codeface-export-{nonce}"));
        let themes = root.join("themes");
        let exports = root.join("exports");
        let theme_root = themes.join("export-test");
        fs::create_dir_all(&theme_root).expect("create theme");
        fs::write(
            theme_root.join("theme.json"),
            r##"{
                "id":"export-test",
                "name":"Export Test",
                "image":"background.png",
                "colors":{"background":"#111111","text":"#f5f5f5"},
                "codexthemes":{"version":"2.3.4"}
            }"##,
        )
        .expect("write manifest");
        fs::write(
            theme_root.join("codeface.css"),
            ".hero { background-image: var(--codeface-art); }",
        )
        .expect("write CSS");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
            .save(theme_root.join("background.png"))
            .expect("write artwork");

        let path = export_theme_from_roots("export-test", &themes, &exports)
            .expect("export theme package");
        let package: Value =
            serde_json::from_str(&fs::read_to_string(path).expect("read exported theme package"))
                .expect("parse exported theme package");
        assert_eq!(package["format"], "codex-theme");
        assert_eq!(package["schemaVersion"], 1);
        assert_eq!(package["manifest"]["id"], "export-test");
        assert_eq!(package["manifest"]["version"], "2.3.4");
        assert!(
            package["css"]
                .as_str()
                .is_some_and(|css| css.contains("assets/artwork.png"))
        );
        let artwork = package["art"]["base64"].as_str().expect("encoded artwork");
        let decoded = STANDARD.decode(artwork).expect("decode artwork");
        assert_eq!(&decoded[..8], b"\x89PNG\r\n\x1a\n");
        fs::remove_dir_all(root).expect("remove export root");
    }

    #[test]
    fn installs_codexthemes_package_as_codeface_theme() {
        let root = std::env::temp_dir().join(format!(
            "codeface-codexthemes-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode artwork");
        let mut package = serde_json::json!({
            "format": "codex-theme",
            "schemaVersion": 1,
            "manifest": {
                "id": "market-test",
                "displayName": "Market Test",
                "description": "Downloaded theme",
                "version": "1.0.0",
                "css": "theme.css",
                "art": "assets/art.png",
                "palette": {
                    "canvas": "#101112",
                    "surface": "#202122",
                    "raised": "#303132",
                    "text": "#F0F1F2",
                    "muted": "#A0A1A2",
                    "accent": "#33CC99",
                    "border": "#404142"
                },
                "design": { "artFocalPoint": "82% 58%", "backgroundScope": "home" }
            },
            "css": ":root[data-codexthemes-theme=\"market-test\"] { --ct-art: url(\"./assets/art.png\"); }",
            "art": {
                "filename": "art.png",
                "mimeType": "image/png",
                "base64": STANDARD.encode(&png)
            }
        });
        let id = install_codexthemes_package_into(
            &serde_json::to_vec(&package).expect("encode package"),
            "market-test",
            &root,
        )
        .expect("install package");
        assert_eq!(id, "market-test");
        let installed = root.join(&id);
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(installed.join("theme.json")).expect("read manifest"),
        )
        .expect("parse manifest");
        assert_eq!(manifest["name"], "Market Test");
        assert_eq!(manifest["colors"]["accent"], "#33CC99");
        assert_eq!(manifest["layout"]["backgroundPosition"], "82% 58%");
        assert_eq!(manifest["codexthemes"]["version"], "1.0.0");
        assert!(
            fs::read_to_string(installed.join("codeface.css"))
                .expect("read CSS")
                .contains("var(--codeface-art)")
        );
        image::open(installed.join("background.png")).expect("decode installed artwork");
        package["manifest"]["displayName"] = Value::String("Market Test Updated".into());
        install_codexthemes_package_into(
            &serde_json::to_vec(&package).expect("encode updated package"),
            "market-test",
            &root,
        )
        .expect("update installed market package");
        let updated: Value = serde_json::from_str(
            &fs::read_to_string(installed.join("theme.json")).expect("read updated manifest"),
        )
        .expect("parse updated manifest");
        assert_eq!(updated["name"], "Market Test Updated");
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn creates_white_fallback_background() {
        let path = std::env::temp_dir().join(format!("codeface-white-{}.png", std::process::id()));
        write_background(None, &path).expect("write white background");
        let image = image::open(&path)
            .expect("read white background")
            .to_rgba8();
        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.get_pixel(0, 0).0, [255, 255, 255, 255]);
        fs::remove_file(path).expect("remove background");
    }

    #[test]
    fn installs_all_bundled_themes_and_overwrites_matching_ids() {
        let root = std::env::temp_dir().join(format!(
            "codeface-bundled-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        install_bundled_themes_into(&root).expect("install bundled theme");
        assert!(!BUNDLED_THEMES.is_empty());
        for theme in BUNDLED_THEMES {
            let theme_root = root.join(theme.id);
            let json: Value = serde_json::from_str(
                &fs::read_to_string(theme_root.join("theme.json")).expect("read manifest"),
            )
            .expect("parse manifest");
            assert_eq!(json["id"], theme.id);
            assert_eq!(json["image"], "background.png");
            assert_eq!(
                json["suggestions"].as_array().map(Vec::len),
                Some(0),
                "bundled themes should not render home suggestions"
            );
            assert!(
                !json["name"]
                    .as_str()
                    .expect("theme name")
                    .to_ascii_lowercase()
                    .starts_with("todo")
            );
            assert!(theme_root.join("codeface.css").is_file());
            image::open(theme_root.join("background.png")).expect("decode background");
            if theme.avatar.is_some() {
                assert_eq!(json["avatar"], "avatar.png");
                image::open(theme_root.join("avatar.png")).expect("decode avatar");
            }
        }

        let theme_root = root.join(BUNDLED_THEMES[0].id);
        fs::write(theme_root.join("codeface.css"), "/* user edit */").expect("edit theme");
        fs::write(theme_root.join("background.png"), b"user background").expect("edit background");
        fs::write(theme_root.join("theme.json"), r#"{"name":"User edit"}"#).expect("edit manifest");

        let custom_root = root.join("custom-theme-id");
        fs::create_dir_all(&custom_root).expect("create custom theme");
        fs::write(custom_root.join("codeface.css"), "/* custom copy */")
            .expect("write custom theme");

        install_bundled_themes_into(&root).expect("second install");
        assert_eq!(
            fs::read_to_string(theme_root.join("codeface.css")).expect("read edit"),
            BUNDLED_THEMES[0].css
        );
        assert_eq!(
            fs::read(theme_root.join("background.png")).expect("read background"),
            BUNDLED_THEMES[0].background
        );
        let json: Value = serde_json::from_str(
            &fs::read_to_string(theme_root.join("theme.json")).expect("read manifest"),
        )
        .expect("parse manifest");
        assert_eq!(json["id"], BUNDLED_THEMES[0].id);
        assert_eq!(
            fs::read_to_string(custom_root.join("codeface.css")).expect("read custom theme"),
            "/* custom copy */"
        );
        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn context_prompt_uses_the_canonical_skill_and_theme_path() {
        let root = std::env::temp_dir().join(format!("codeface-prompt-{}", std::process::id()));
        fs::create_dir_all(&root).expect("create theme");
        fs::write(
            root.join("theme.json"),
            r#"{"name":"Prompt Test","image":"background.png"}"#,
        )
        .expect("write json");
        fs::write(root.join("codeface.css"), "html.codeface { color: red; }").expect("write css");
        write_background(None, &root.join("background.png")).expect("write image");
        let prompt = context_prompt_for_root(&root, false).expect("build prompt");
        assert!(prompt.contains(&root.display().to_string()));
        assert!(prompt.contains(CODEFACE_SKILL_URL));
        assert!(prompt.contains("Read and follow the CodeFace Skill first"));
        assert!(!prompt.contains("Directory files"));
        assert!(!prompt.contains("Background image"));
        assert!(!prompt.contains("Implementation requirements"));

        let chinese_prompt = context_prompt_for_root(&root, true).expect("build Chinese prompt");
        assert!(chinese_prompt.contains(&root.display().to_string()));
        assert!(chinese_prompt.contains(CODEFACE_SKILL_URL));
        assert!(chinese_prompt.contains("请先读取并遵循 CodeFace Skill"));
        assert!(!chinese_prompt.contains("目录文件"));
        assert!(!chinese_prompt.contains("背景图片"));
        assert!(!chinese_prompt.contains("实现要求"));
        fs::remove_dir_all(root).expect("remove theme");
    }

    #[test]
    fn new_theme_name_uses_date_and_next_available_version() {
        let root = std::env::temp_dir().join(format!(
            "codeface-new-theme-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(root.join("theme-20260719-v1")).expect("create existing theme");

        let json = new_theme_json_for_root(&root, "20260719").expect("generate theme JSON");
        let value: Value = serde_json::from_str(&json).expect("parse generated JSON");
        assert_eq!(value["id"], "theme-20260719-v2");
        assert_eq!(value["name"], "theme-20260719-v2");

        fs::remove_dir_all(root).expect("remove test root");
    }

    #[test]
    fn system_theme_cannot_be_deleted() {
        let error =
            delete("__codeface-system-theme__").expect_err("system theme must be protected");
        assert!(error.to_string().contains("cannot be deleted"));
    }

    #[test]
    fn delete_removes_theme_directory() {
        let root = std::env::temp_dir().join(format!(
            "codeface-delete-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let theme_root = root.join("deletable-theme");
        fs::create_dir_all(&theme_root).expect("create theme directory");
        fs::write(theme_root.join("theme.json"), r#"{"name":"Delete Me"}"#)
            .expect("write theme file");

        delete_from_root(&root, "deletable-theme").expect("delete theme");

        assert!(!theme_root.exists());
        fs::remove_dir_all(root).expect("remove test root");
    }
}
