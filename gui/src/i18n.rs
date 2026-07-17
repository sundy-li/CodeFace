use crate::paths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    System,
    English,
    SimplifiedChinese,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    English,
    SimplifiedChinese,
}

#[derive(Default, Deserialize, Serialize)]
struct Settings {
    language: Language,
}

impl Language {
    pub fn effective(self) -> Locale {
        match self {
            Self::English => Locale::English,
            Self::SimplifiedChinese => Locale::SimplifiedChinese,
            Self::System => {
                let locale = sys_locale::get_locale()
                    .unwrap_or_else(|| "en".into())
                    .to_ascii_lowercase();
                if locale.starts_with("zh") {
                    Locale::SimplifiedChinese
                } else {
                    Locale::English
                }
            }
        }
    }
}

pub fn load() -> Language {
    paths::settings_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .map(|settings| settings.language)
        .unwrap_or_default()
}

pub fn save(language: Language) -> Result<()> {
    let path = paths::settings_path()?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(
        &temporary,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&Settings { language })?
        ),
    )?;
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn t(locale: Locale, key: &str) -> &'static str {
    match (locale, key) {
        (Locale::SimplifiedChinese, "ready") => "准备就绪",
        (Locale::English, "ready") => "Ready",
        (Locale::SimplifiedChinese, "busy") => "已有操作正在执行，请稍候",
        (Locale::English, "busy") => "Another operation is already running",
        (Locale::SimplifiedChinese, "themes_empty") => "还没有主题，请新增或导入主题",
        (Locale::English, "themes_empty") => "No themes yet. Create or import one.",
        (Locale::SimplifiedChinese, "new_source_ready") => {
            "新主题源码已就绪；未选择图片时使用纯白背景"
        }
        (Locale::English, "new_source_ready") => {
            "New theme source is ready; white is used when no image is selected"
        }
        (Locale::SimplifiedChinese, "source_loaded") => "已载入主题源码",
        (Locale::English, "source_loaded") => "Theme source loaded",
        (Locale::SimplifiedChinese, "saved") => "主题已保存",
        (Locale::English, "saved") => "Theme saved",
        (Locale::SimplifiedChinese, "saved_applied") => "主题已保存并切换预览",
        (Locale::English, "saved_applied") => "Theme saved and applied",
        (Locale::SimplifiedChinese, "pack_imported") => "主题包已导入",
        (Locale::English, "pack_imported") => "Theme package imported",
        (Locale::SimplifiedChinese, "codex_closed") => "Codex 已关闭",
        (Locale::English, "codex_closed") => "Codex closed",
        (Locale::SimplifiedChinese, "codex_restarted") => {
            "Codex 已按官方模式重启；应用主题时会重新建立本机 CDP 会话"
        }
        (Locale::English, "codex_restarted") => {
            "Codex restarted normally; applying a theme will create a local CDP session"
        }
        (Locale::SimplifiedChinese, "white_background") => "纯白背景（尚未选择图片）",
        (Locale::English, "white_background") => "White background (no image selected)",
        (Locale::SimplifiedChinese, "edit_source") => "编辑主题源码",
        (Locale::English, "edit_source") => "Edit theme source",
        (Locale::SimplifiedChinese, "new_source") => "新建主题源码",
        (Locale::English, "new_source") => "New theme source",
        (Locale::SimplifiedChinese, "back") => "返回管理",
        (Locale::English, "back") => "Back",
        (Locale::SimplifiedChinese, "choose_image") => "选择背景图",
        (Locale::English, "choose_image") => "Choose image",
        (Locale::SimplifiedChinese, "save") => "保存主题",
        (Locale::English, "save") => "Save theme",
        (Locale::SimplifiedChinese, "save_apply") => "保存并预览",
        (Locale::English, "save_apply") => "Save and preview",
        (Locale::SimplifiedChinese, "new_task") => "新建任务",
        (Locale::English, "new_task") => "New task",
        (Locale::SimplifiedChinese, "scheduled") => "已安排",
        (Locale::English, "scheduled") => "Scheduled",
        (Locale::SimplifiedChinese, "plugins") => "插件",
        (Locale::English, "plugins") => "Plugins",
        (Locale::SimplifiedChinese, "pull_requests") => "拉取请求",
        (Locale::English, "pull_requests") => "Pull requests",
        (Locale::SimplifiedChinese, "projects") => "项目",
        (Locale::English, "projects") => "Projects",
        (Locale::SimplifiedChinese, "hero") => "我们应该构建什么？",
        (Locale::English, "hero") => "What should we build?",
        (Locale::SimplifiedChinese, "preview_caption") => "CodeFace 主题预览",
        (Locale::English, "preview_caption") => "CodeFace theme preview",
        (Locale::SimplifiedChinese, "composer") => "随心输入…",
        (Locale::English, "composer") => "Ask anything…",
        (Locale::SimplifiedChinese, "select_theme") => "请选择一个主题",
        (Locale::English, "select_theme") => "Select a theme",
        (Locale::SimplifiedChinese, "theme_preview") => "主题预览",
        (Locale::English, "theme_preview") => "Theme preview",
        (Locale::SimplifiedChinese, "apply_theme") => "应用主题",
        (Locale::English, "apply_theme") => "Apply theme",
        (Locale::SimplifiedChinese, "subtitle") => "原生 GPUI 主题管理器 · CDP 仅在应用主题时启用",
        (Locale::English, "subtitle") => {
            "Native GPUI theme manager · CDP is enabled only when applying themes"
        }
        (Locale::SimplifiedChinese, "close_codex") => "关闭 Codex",
        (Locale::English, "close_codex") => "Close Codex",
        (Locale::SimplifiedChinese, "restart_codex") => "重启 Codex",
        (Locale::English, "restart_codex") => "Restart Codex",
        (Locale::SimplifiedChinese, "settings") => "设置",
        (Locale::English, "settings") => "Settings",
        (Locale::SimplifiedChinese, "theme_library") => "主题库",
        (Locale::English, "theme_library") => "Theme library",
        (Locale::SimplifiedChinese, "new_theme") => "新增主题",
        (Locale::English, "new_theme") => "New theme",
        (Locale::SimplifiedChinese, "import_pack") => "导入主题包",
        (Locale::English, "import_pack") => "Import package",
        (Locale::SimplifiedChinese, "double_click") => "双击主题可编辑源码",
        (Locale::English, "double_click") => "Double-click a theme to edit its source",
        (Locale::SimplifiedChinese, "language") => "语言",
        (Locale::English, "language") => "Language",
        (Locale::SimplifiedChinese, "language_help") => {
            "未指定时自动跟随系统语言。更改后立即生效。"
        }
        (Locale::English, "language_help") => {
            "When unset, CodeFace follows the system language. Changes apply immediately."
        }
        (Locale::SimplifiedChinese, "follow_system") => "跟随系统",
        (Locale::English, "follow_system") => "Follow system",
        (_, "english") => "English",
        (_, "simplified_chinese") => "简体中文",
        (Locale::SimplifiedChinese, "settings_saved") => "语言设置已保存",
        (Locale::English, "settings_saved") => "Language setting saved",
        (Locale::SimplifiedChinese, "operation_failed") => "操作失败",
        (Locale::English, "operation_failed") => "Operation failed",
        (Locale::SimplifiedChinese, "image_selected") => "已选择背景图",
        (Locale::English, "image_selected") => "Background selected",
        (Locale::SimplifiedChinese, "applied") => "已应用",
        (Locale::English, "applied") => "Applied",
        (Locale::SimplifiedChinese, "verify_ok") => "CodeFace 已验证通过",
        (Locale::English, "verify_ok") => "CodeFace verified",
        (Locale::SimplifiedChinese, "restored") => "已恢复官方界面",
        (Locale::English, "restored") => "Official appearance restored",
        (Locale::SimplifiedChinese, "image_filter") => "图片",
        (Locale::English, "image_filter") => "Images",
        (Locale::SimplifiedChinese, "select_image_dialog") => "选择主题背景图",
        (Locale::English, "select_image_dialog") => "Select theme background",
        (Locale::SimplifiedChinese, "select_pack_dialog") => "选择主题包目录",
        (Locale::English, "select_pack_dialog") => "Select theme package folder",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_languages_are_deterministic() {
        assert_eq!(Language::English.effective(), Locale::English);
        assert_eq!(
            Language::SimplifiedChinese.effective(),
            Locale::SimplifiedChinese
        );
        assert_eq!(t(Locale::English, "settings"), "Settings");
        assert_eq!(t(Locale::SimplifiedChinese, "settings"), "设置");
    }

    #[test]
    fn language_values_round_trip() {
        let json = serde_json::to_string(&Language::SimplifiedChinese).expect("serialize");
        assert_eq!(json, "\"simplified-chinese\"");
        assert_eq!(
            serde_json::from_str::<Language>(&json).expect("deserialize"),
            Language::SimplifiedChinese
        );
    }
}
