use crate::paths;
use anyhow::{Context, Result, bail};
use chrono::Local;
use image::{DynamicImage, Rgba, RgbaImage};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_JSON: &str = include_str!("../../resources/theme-pack-template/theme.json");
pub const DEFAULT_CSS: &str = include_str!("../../resources/theme-pack-template/codeface.css");

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
    delete_from_root(&paths::themes_root()?, id)
}

pub fn context_prompt(id: &str, chinese: bool) -> Result<String> {
    let root = paths::themes_root()?.join(id);
    context_prompt_for_root(&root, chinese)
}

fn context_prompt_for_root(root: &Path, chinese: bool) -> Result<String> {
    let json = fs::read_to_string(root.join("theme.json"))?;
    fs::metadata(root.join("codeface.css"))?;
    let value: Value = serde_json::from_str(&json)?;
    let image_name = value
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or("background.png");
    let image_path = root.join(image_name);
    let metadata = fs::metadata(&image_path)?;
    let dimensions = image::image_dimensions(&image_path).ok();
    let dimension_text = dimensions
        .map(|(width, height)| format!("{width} × {height}"))
        .unwrap_or_else(|| "unknown".into());
    let mut files = fs::read_dir(root)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    files.sort();
    let files = files
        .into_iter()
        .map(|name| format!("- {name}"))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = if chinese {
        format!(
            r#"请帮我完整优化一个 CodeFace 主题。你可以直接读取和编辑下面的本地主题目录。请先检查现有主题和背景图，再进行有依据的设计调整；不要只给建议，请直接完成修改和验证。

主题目录：{root}

目录文件：
{files}

背景图片：
- 路径：{image_path}
- 尺寸：{dimension_text}
- 文件大小：{image_bytes} bytes

设计范围：
1. 不要只优化首页。必须同时检查首页、左侧导航栏、聊天/任务页面、消息内容区、顶部栏和输入框。
2. 从背景图片中提取协调的背景色、面板色、强调色、文字色和边框色，让所有页面属于同一视觉系统。
3. 首页可以使用背景图片；聊天和任务页面应优先使用与图片协调的纯色或低对比渐变，避免重复铺设人物或复杂图片影响阅读。
4. 保持清晰的信息层级、正文可读性、足够的颜色对比度，以及 hover、pressed、selected、focus-visible 和 reduced-motion 状态。
5. 保留所有真实 Codex 控件和交互。装饰层必须不可交互，不得遮挡、替换或隐藏原生功能。

实现要求：
1. 直接编辑主题目录中的 `theme.json` 和 `codeface.css`；仅在确有必要时替换背景图片。
2. 不要修改 CodeFace 源码或官方 Codex 应用。
3. 保持 JSON 的 `image` 字段与实际图片文件一致。
4. CSS 中不得使用 `@import`、`@font-face` 或任何外部 `url(...)` 资源。
5. CSS 必须保持在 256 KiB 以内，图片必须保持在 16 MiB 以内。
6. 修改完成后验证 JSON、CSS、图片引用和文件大小，并确认主题包仍可导入。
7. 最后总结修改过的文件、主要设计决策和验证结果。
"#,
            root = root.display(),
            image_path = image_path.display(),
            image_bytes = metadata.len()
        )
    } else {
        format!(
            r#"Help me fully refine a CodeFace theme. You can read and edit the local theme directory below. Inspect the existing theme and background image before making evidence-based design changes. Do not stop at recommendations: edit the files and verify the result.

Theme directory: {root}

Directory files:
{files}

Background image:
- Path: {image_path}
- Dimensions: {dimension_text}
- File size: {image_bytes} bytes

Design scope:
1. Do not optimize only the home screen. Review the home screen, left navigation, chat/task pages, message content, top bar, and composer.
2. Derive coordinated canvas, panel, accent, text, and border colors from the background image so every route belongs to one visual system.
3. The home screen may use the image. Prefer image-related solid colors or low-contrast gradients on chat and task pages; do not repeat a portrait or detailed image where it would impair reading.
4. Preserve clear hierarchy, readable text, sufficient contrast, and distinct hover, pressed, selected, focus-visible, and reduced-motion states.
5. Preserve every real Codex control and interaction. Decorative layers must be non-interactive and must never obscure, replace, or hide native functionality.

Implementation requirements:
1. Edit `theme.json` and `codeface.css` directly in the theme directory; replace the background image only when necessary.
2. Do not modify the CodeFace source code or the official Codex application.
3. Keep the JSON `image` field consistent with the actual image file.
4. Do not use `@import`, `@font-face`, or external `url(...)` resources in CSS.
5. Keep CSS below 256 KiB and the background image below 16 MiB.
6. Validate JSON, CSS, the image reference, and file-size limits, then confirm that the package remains importable.
7. Finish with a concise summary of changed files, key design decisions, and verification results.
"#,
            root = root.display(),
            image_path = image_path.display(),
            image_bytes = metadata.len()
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
    fn context_prompt_contains_complete_editable_context() {
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
        assert!(prompt.contains("- theme.json"));
        assert!(prompt.contains("- codeface.css"));
        assert!(prompt.contains("1 × 1"));
        assert!(prompt.contains("left navigation"));
        assert!(prompt.contains("chat/task pages"));
        assert!(prompt.contains("focus-visible"));
        assert!(prompt.contains("256 KiB"));

        let chinese_prompt = context_prompt_for_root(&root, true).expect("build Chinese prompt");
        assert!(chinese_prompt.contains("左侧导航栏"));
        assert!(chinese_prompt.contains("聊天/任务页面"));
        assert!(chinese_prompt.contains("不要修改 CodeFace 源码"));
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
