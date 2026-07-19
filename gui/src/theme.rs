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
