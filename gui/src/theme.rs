use crate::paths;
use anyhow::{Context, Result, bail};
use image::{DynamicImage, Rgba, RgbaImage};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_JSON: &str = include_str!("../../resources/theme-pack-template/theme.json");
pub const DEFAULT_CSS: &str = include_str!("../../resources/theme-pack-template/codeface.css");

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("目标文件没有父目录")?;
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
    let value: Value = serde_json::from_str(source).context("theme.json 不是有效 JSON")?;
    let object = value.as_object().context("theme.json 顶层必须是对象")?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if name.is_empty() {
        bail!("theme.json 必须包含非空 name");
    }
    Ok(value)
}

fn write_background(source: Option<&Path>, target: &Path) -> Result<()> {
    let image = match source {
        Some(path) => {
            image::open(path).with_context(|| format!("无法读取背景图 {}", path.display()))?
        }
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
        bail!("codeface.css 不能超过 256 KiB");
    }
    let normalized_css = css.to_ascii_lowercase();
    if ["@import", "@font-face", "url("]
        .iter()
        .any(|token| normalized_css.contains(token))
    {
        bail!("codeface.css 不允许加载外部字体、导入文件或 URL 资源");
    }
    let mut value = validate_json(json)?;
    let name = value["name"].as_str().unwrap();
    let id = existing_id
        .map(str::to_owned)
        .unwrap_or_else(|| safe_id(value.get("id").and_then(Value::as_str).unwrap_or(name)));
    let root = paths::themes_root()?.join(&id);
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

pub fn import_directory(source: &Path) -> Result<String> {
    let json = fs::read_to_string(source.join("theme.json")).context("主题包缺少 theme.json")?;
    let css = fs::read_to_string(source.join("codeface.css")).context("主题包缺少 codeface.css")?;
    let value = validate_json(&json)?;
    let image_name = value
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let image = source.join(image_name);
    if !image.is_file() {
        bail!("主题包背景图不存在: {image_name}");
    }
    save(&json, &css, Some(&image), None)
}

pub fn activate(id: &str) -> Result<PathBuf> {
    if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        bail!("非法主题 ID");
    }
    let source = paths::themes_root()?.join(id);
    if !source.join("theme.json").is_file() {
        bail!("主题不存在: {id}");
    }
    let target = paths::active_theme_root()?;
    for entry in fs::read_dir(&target)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    for name in ["theme.json", "codeface.css", "background.png"] {
        fs::copy(source.join(name), target.join(name))
            .with_context(|| format!("复制主题文件 {name} 失败"))?;
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

pub fn choose_image(title: &str, filter_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter(filter_name, &["png", "jpg", "jpeg", "webp"])
        .pick_file()
}

pub fn choose_pack(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_theme_and_normalizes_id() {
        let value = validate_json(r#"{"name":"中文主题"}"#).expect("valid theme");
        assert_eq!(value["name"], "中文主题");
        assert_eq!(safe_id("CodeFace 01"), "codeface-01");
        assert!(validate_json("{}").is_err());
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
}
