use crate::paths;
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Local;
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const CODEXTHEMES_BASE_URL: &str = "https://codexthemes.ai";
const CODEXTHEMES_MAX_PACKAGE_SIZE: usize = 30 * 1024 * 1024;
const CODEXTHEMES_MAX_PREVIEW_SIZE: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarketTheme {
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub id: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub description: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub author: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub mode: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub image: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub url: String,
    #[serde(default, deserialize_with = "deserialize_null_string")]
    pub kind: String,
    pub installable: bool,
    #[serde(
        rename = "downloadUrl",
        default,
        deserialize_with = "deserialize_null_string"
    )]
    pub download_url: String,
    #[serde(default)]
    pub verified: bool,
}

impl MarketTheme {
    pub fn can_install(&self) -> bool {
        self.installable || self.kind == "theme"
    }
}

fn deserialize_null_string<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct MarketResponse {
    #[serde(default)]
    themes: Vec<MarketTheme>,
}

impl MarketResponse {
    fn into_valid_themes(self) -> Vec<MarketTheme> {
        self.themes
            .into_iter()
            .filter(|theme| !theme.id.trim().is_empty() && !theme.name.trim().is_empty())
            .collect()
    }
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
    preview: &'static [u8],
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
        value["preview"] = Value::String("preview.png".into());
        if theme.avatar.is_some() {
            value["avatar"] = Value::String("avatar.png".into());
        }
        atomic_write(
            &root.join("theme.json"),
            format!("{}\n", serde_json::to_string_pretty(&value)?).as_bytes(),
        )?;
        atomic_write(&root.join("codeface.css"), theme.css.as_bytes())?;
        atomic_write(&root.join("background.png"), theme.background)?;
        atomic_write(&root.join("preview.png"), theme.preview)?;
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
    let art = package.get("art").and_then(Value::as_object);
    if art.is_none() && manifest.get("art").is_some() {
        bail!("CodexThemes manifest references missing artwork");
    }
    let art_name = art
        .and_then(|art| art.get("filename"))
        .and_then(Value::as_str)
        .unwrap_or("background.png")
        .to_owned();
    if Path::new(&art_name)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(art_name.as_str())
    {
        bail!("CodexThemes artwork filename must be a plain relative filename");
    }
    let manifest_art = manifest
        .get("art")
        .and_then(Value::as_str)
        .unwrap_or(&art_name);
    let css = rewrite_codexthemes_art_urls(css, &[manifest_art, &art_name])?;
    let artwork = if let Some(art) = art {
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
        image::load_from_memory(&art_bytes).context("failed to decode CodexThemes artwork")?
    } else {
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([255, 255, 255, 255])))
    };

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
        .query(&[("q", query.trim()), ("limit", "100")])
        .send()
        .context("failed to search CodexThemes")?
        .error_for_status()
        .context("CodexThemes search failed")?
        .json()
        .context("CodexThemes search response is invalid")?;
    Ok(response.into_valid_themes())
}

fn download_market_preview_into(
    market_theme: &MarketTheme,
    previews_root: &Path,
) -> Result<PathBuf> {
    codexthemes_id(&market_theme.id)?;
    let source =
        reqwest::Url::parse(&market_theme.image).context("CodexThemes preview URL is invalid")?;
    validate_codexthemes_https_url(&source, "preview")?;
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(source)
        .header(reqwest::header::ACCEPT, "image/png,image/jpeg,image/webp")
        .send()
        .context("failed to download CodexThemes preview")?
        .error_for_status()
        .context("CodexThemes preview download failed")?;
    let final_url = response.url();
    validate_codexthemes_https_url(final_url, "preview redirect")?;
    if response
        .content_length()
        .is_some_and(|length| length > CODEXTHEMES_MAX_PREVIEW_SIZE as u64)
    {
        bail!("CodexThemes preview cannot exceed 8 MiB");
    }
    let mut bytes = Vec::new();
    response
        .take(CODEXTHEMES_MAX_PREVIEW_SIZE as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > CODEXTHEMES_MAX_PREVIEW_SIZE {
        bail!("CodexThemes preview cannot exceed 8 MiB");
    }
    let png = normalize_market_preview(&bytes)?;
    fs::create_dir_all(previews_root)?;
    let path = previews_root.join(format!("{}.png", market_theme.id));
    atomic_write(&path, &png)?;
    Ok(path)
}

fn validate_codexthemes_https_url(url: &reqwest::Url, label: &str) -> Result<()> {
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https" || !(host == "codexthemes.ai" || host.ends_with(".codexthemes.ai")) {
        bail!("CodexThemes {label} must use a trusted HTTPS domain");
    }
    Ok(())
}

fn normalize_market_preview(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .context("failed to detect CodexThemes preview format")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .context("failed to decode CodexThemes preview")?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        bail!("CodexThemes preview dimensions are unsupported");
    }
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    if png.len() > CODEXTHEMES_MAX_PREVIEW_SIZE {
        bail!("normalized CodexThemes preview cannot exceed 8 MiB");
    }
    Ok(png)
}

pub fn download_market_preview(market_theme: &MarketTheme) -> Result<PathBuf> {
    download_market_preview_into(market_theme, &paths::market_previews_root()?)
}

pub fn preview_from_codexthemes(input: &str) -> Result<(MarketTheme, PathBuf)> {
    let id = codexthemes_id(input)?;
    let market_theme = search_codexthemes(&id)?
        .into_iter()
        .find(|theme| theme.id == id)
        .with_context(|| format!("CodexThemes theme was not found: {id}"))?;
    let path = download_market_preview(&market_theme)?;
    Ok((market_theme, path))
}

fn package_url_from_detail_html(html: &str) -> Result<reqwest::Url> {
    let tail = ["\"packageUrl\":\"", "packageUrl:\""]
        .into_iter()
        .find_map(|marker| html.split_once(marker).map(|(_, tail)| tail))
        .context("CodexThemes detail page does not expose a package URL")?;
    let end = tail
        .find('"')
        .context("CodexThemes detail page package URL is malformed")?;
    let encoded = format!("\"{}\"", &tail[..end]);
    let value: String =
        serde_json::from_str(&encoded).context("CodexThemes detail page package URL is invalid")?;
    let url = reqwest::Url::parse(&value).context("CodexThemes package URL is invalid")?;
    validate_codexthemes_https_url(&url, "package")?;
    Ok(url)
}

fn download_manual_codexthemes_package(id: &str) -> Result<Vec<u8>> {
    let detail_url = format!("{CODEXTHEMES_BASE_URL}/themes/{id}");
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(detail_url)
        .header(reqwest::header::ACCEPT, "text/html")
        .send()
        .context("failed to load CodexThemes theme details")?
        .error_for_status()
        .context("CodexThemes theme detail request failed")?;
    validate_codexthemes_https_url(response.url(), "detail page")?;
    let mut html = Vec::new();
    response.take(2 * 1024 * 1024 + 1).read_to_end(&mut html)?;
    if html.len() > 2 * 1024 * 1024 {
        bail!("CodexThemes detail page cannot exceed 2 MiB");
    }
    let html = std::str::from_utf8(&html).context("CodexThemes detail page is not UTF-8")?;
    let package_url = package_url_from_detail_html(html)?;
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .get(package_url)
        .header(reqwest::header::ACCEPT, "application/json,application/zip")
        .send()
        .context("failed to download CodexThemes manual package")?
        .error_for_status()
        .context("CodexThemes manual package download failed")?;
    validate_codexthemes_https_url(response.url(), "package redirect")?;
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

fn archive_codexthemes_package(bytes: &[u8], requested_id: &str) -> Result<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .context("CodexThemes manual package is not a valid ZIP archive")?;
    if archive.len() > 256 {
        bail!("CodexThemes archive cannot contain more than 256 entries");
    }
    let mut files = HashMap::<String, Vec<u8>>::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let enclosed = entry
            .enclosed_name()
            .context("CodexThemes archive contains an unsafe path")?;
        let name = enclosed.to_string_lossy().replace('\\', "/");
        total = total.saturating_add(entry.size());
        if total > CODEXTHEMES_MAX_PACKAGE_SIZE as u64 {
            bail!("CodexThemes archive expands beyond 30 MiB");
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(CODEXTHEMES_MAX_PACKAGE_SIZE as u64 + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 != entry.size() {
            bail!("CodexThemes archive entry exceeds its declared size");
        }
        files.insert(name, data);
    }
    let manifest_path = files
        .keys()
        .find(|name| name.as_str() == "theme.json" || name.ends_with("/theme.json"))
        .cloned()
        .context("CodexThemes archive is missing theme.json")?;
    let manifest: Value = serde_json::from_slice(&files[&manifest_path])
        .context("CodexThemes archive theme.json is invalid")?;
    if manifest.get("id").and_then(Value::as_str) != Some(requested_id) {
        bail!("CodexThemes archive ID does not match the requested theme");
    }
    let root = manifest_path.strip_suffix("theme.json").unwrap_or_default();
    let relative_file = |key: &str| -> Result<(&str, &Vec<u8>)> {
        let relative = manifest
            .get(key)
            .and_then(Value::as_str)
            .with_context(|| format!("CodexThemes archive manifest is missing {key}"))?;
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            bail!("CodexThemes archive manifest contains an unsafe {key} path");
        }
        let full = format!("{root}{}", relative.trim_start_matches("./"));
        let data = files
            .get(&full)
            .with_context(|| format!("CodexThemes archive is missing {relative}"))?;
        Ok((relative, data))
    };
    let (_, css_bytes) = relative_file("css")?;
    let (art_name, art_bytes) = relative_file("art")?;
    let css = std::str::from_utf8(css_bytes).context("CodexThemes archive CSS is not UTF-8")?;
    let art_filename = Path::new(art_name)
        .file_name()
        .and_then(|name| name.to_str())
        .context("CodexThemes archive artwork filename is invalid")?;
    let package = serde_json::json!({
        "format": "codex-theme",
        "schemaVersion": 1,
        "manifest": manifest,
        "css": css,
        "art": {
            "filename": art_filename,
            "mimeType": "image/png",
            "base64": STANDARD.encode(art_bytes)
        }
    });
    Ok(serde_json::to_vec(&package)?)
}

fn normalize_codexthemes_package(bytes: Vec<u8>, requested_id: &str) -> Result<Vec<u8>> {
    if bytes.starts_with(b"PK\x03\x04") {
        archive_codexthemes_package(&bytes, requested_id)
    } else {
        Ok(bytes)
    }
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
    if response.url().path().starts_with("/sign-in") {
        return normalize_codexthemes_package(download_manual_codexthemes_package(id)?, id);
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
    normalize_codexthemes_package(bytes, id)
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

pub fn import_codextheme_package(bytes: &[u8]) -> Result<String> {
    if bytes.len() > CODEXTHEMES_MAX_PACKAGE_SIZE {
        bail!("CodexThemes package cannot exceed 30 MiB");
    }
    let package: Value =
        serde_json::from_slice(bytes).context("CodexThemes package is not valid UTF-8 JSON")?;
    let id = package
        .pointer("/manifest/id")
        .and_then(Value::as_str)
        .context("CodexThemes package manifest is missing its ID")?;
    let id = codexthemes_id(id)?;
    install_codexthemes_package_into(bytes, &id, &paths::themes_root()?)
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
    use std::io::Write;

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
            }, {
                "id": "showcase-only",
                "name": "Showcase only",
                "description": null,
                "author": null,
                "mode": null,
                "image": null,
                "url": "https://codexthemes.ai/themes/showcase-only",
                "kind": null,
                "installable": false,
                "downloadUrl": null
            }, {
                "id": null,
                "name": null,
                "url": null,
                "kind": null,
                "installable": false
            }]
        }))
        .expect("parse market response");
        let themes = response.into_valid_themes();
        assert_eq!(themes.len(), 2);
        assert!(themes[0].installable);
        assert_eq!(
            themes[0].download_url,
            "https://codexthemes.ai/api/themes/coast/download"
        );
        assert!(themes[1].download_url.is_empty());
        assert!(themes[1].description.is_empty());
        assert!(themes[1].author.is_empty());
        assert!(themes[1].mode.is_empty());
        assert!(themes[1].image.is_empty());
        assert!(themes[1].kind.is_empty());
        assert!(!themes[1].can_install());
        let mut manual_theme = themes[0].clone();
        manual_theme.installable = false;
        manual_theme.download_url.clear();
        assert!(manual_theme.can_install());
    }

    #[test]
    fn market_preview_urls_require_trusted_https_hosts() {
        for trusted in [
            "https://codexthemes.ai/preview.png",
            "https://cdn.codexthemes.ai/uploads/preview.webp",
        ] {
            validate_codexthemes_https_url(&reqwest::Url::parse(trusted).expect("URL"), "preview")
                .expect("trusted preview URL");
        }
        for rejected in [
            "http://cdn.codexthemes.ai/preview.png",
            "https://codexthemes.ai.example.com/preview.png",
            "https://example.com/preview.png",
        ] {
            assert!(
                validate_codexthemes_https_url(
                    &reqwest::Url::parse(rejected).expect("URL"),
                    "preview"
                )
                .is_err()
            );
        }
    }

    #[test]
    fn detail_page_package_url_is_extracted_and_restricted() {
        let html = r#"<script>theme={"packageUrl":"https://cdn.codexthemes.ai/uploads/theme.zip"}</script>"#;
        assert_eq!(
            package_url_from_detail_html(html)
                .expect("extract package URL")
                .as_str(),
            "https://cdn.codexthemes.ai/uploads/theme.zip"
        );
        let flight_data = r#"theme={packageUrl:"https://cdn.codexthemes.ai/uploads/manual.zip"}"#;
        assert_eq!(
            package_url_from_detail_html(flight_data)
                .expect("extract flight-data package URL")
                .as_str(),
            "https://cdn.codexthemes.ai/uploads/manual.zip"
        );
        let external = r#"{"packageUrl":"https://example.com/theme.zip"}"#;
        assert!(package_url_from_detail_html(external).is_err());
    }

    #[test]
    fn market_preview_is_normalized_to_png() {
        let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 2, Rgba([12, 34, 56, 255])));
        let mut jpeg = Vec::new();
        source
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .expect("encode preview fixture");
        let png = normalize_market_preview(&jpeg).expect("normalize preview");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(
            image::load_from_memory(&png)
                .expect("decode normalized preview")
                .dimensions(),
            (3, 2)
        );
    }

    #[test]
    fn manual_zip_theme_is_converted_to_codextheme_package() {
        let mut artwork = Vec::new();
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba([3, 4, 5, 255])))
            .write_to(&mut Cursor::new(&mut artwork), image::ImageFormat::Png)
            .expect("encode artwork");
        let manifest = serde_json::json!({
            "schemaVersion": 1,
            "id": "manual-archive",
            "displayName": "Manual Archive",
            "description": "ZIP fixture",
            "version": "1.0.0",
            "css": "theme.css",
            "art": "assets/artwork.png",
            "palette": {"canvas":"#111111","text":"#ffffff"}
        });
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file("manual-archive/theme.json", options)
            .expect("start manifest");
        writer
            .write_all(
                serde_json::to_string(&manifest)
                    .expect("manifest JSON")
                    .as_bytes(),
            )
            .expect("write manifest");
        writer
            .start_file("manual-archive/theme.css", options)
            .expect("start CSS");
        writer
            .write_all(b":root { color: white; }")
            .expect("write CSS");
        writer
            .start_file("manual-archive/assets/artwork.png", options)
            .expect("start artwork");
        writer.write_all(&artwork).expect("write artwork");
        let archive = writer.finish().expect("finish archive").into_inner();

        let package =
            archive_codexthemes_package(&archive, "manual-archive").expect("convert ZIP package");
        let value: Value = serde_json::from_slice(&package).expect("parse converted package");
        assert_eq!(value["format"], "codex-theme");
        assert_eq!(value["manifest"]["id"], "manual-archive");
        assert_eq!(value["css"], ":root { color: white; }");
        assert!(
            !value["art"]["base64"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
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
    fn installs_artless_codexthemes_package_with_plain_fallback() {
        let root = std::env::temp_dir().join(format!(
            "codeface-artless-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let package = serde_json::json!({
            "format": "codex-theme",
            "schemaVersion": 1,
            "manifest": {
                "id": "artless-test",
                "displayName": "Artless Test",
                "version": "1.0.0",
                "css": "theme.css",
                "palette": {"canvas":"#f7fbfe","text":"#071a2c"}
            },
            "css": ":root[data-codexthemes-theme=\"artless-test\"] { color: #071a2c; }"
        });
        install_codexthemes_package_into(
            &serde_json::to_vec(&package).expect("encode package"),
            "artless-test",
            &root,
        )
        .expect("install artless package");
        let image =
            image::open(root.join("artless-test/background.png")).expect("decode fallback artwork");
        assert_eq!(image.dimensions(), (1, 1));
        fs::remove_dir_all(root).expect("remove artless root");
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
            assert_eq!(json["preview"], "preview.png");
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
            image::open(theme_root.join("preview.png")).expect("decode effect preview");
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
        assert_eq!(
            fs::read(theme_root.join("preview.png")).expect("read effect preview"),
            BUNDLED_THEMES[0].preview
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
