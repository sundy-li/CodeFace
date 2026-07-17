use crate::paths;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, sync::LazyLock};

const EN_JSON: &str = include_str!("../../resources/i18n/en.json");
const ZH_CN_JSON: &str = include_str!("../../resources/i18n/zh-CN.json");

static EN: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(EN_JSON).expect("embedded English translations must be valid JSON")
});
static ZH_CN: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(ZH_CN_JSON)
        .expect("embedded Simplified Chinese translations must be valid JSON")
});

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    System,
    English,
    SimplifiedChinese,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Appearance {
    Light,
    #[default]
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Locale {
    English,
    SimplifiedChinese,
}

#[derive(Default, Deserialize, Serialize)]
struct Settings {
    language: Language,
    appearance: Appearance,
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
    load_settings().language
}

pub fn load_appearance() -> Appearance {
    load_settings().appearance
}

fn load_settings() -> Settings {
    paths::settings_path()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default()
}

pub fn save(language: Language, appearance: Appearance) -> Result<()> {
    let path = paths::settings_path()?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(
        &temporary,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&Settings {
                language,
                appearance,
            })?
        ),
    )?;
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn t(locale: Locale, key: &str) -> &str {
    let localized = match locale {
        Locale::English => &*EN,
        Locale::SimplifiedChinese => &*ZH_CN,
    };
    localized
        .get(key)
        .or_else(|| EN.get(key))
        .map(String::as_str)
        .unwrap_or(key)
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
    fn catalogs_are_valid_and_have_identical_keys() {
        assert!(!EN.is_empty());
        assert_eq!(
            EN.keys().collect::<std::collections::BTreeSet<_>>(),
            ZH_CN.keys().collect()
        );
    }

    #[test]
    fn missing_keys_return_the_key() {
        assert_eq!(t(Locale::English, "missing.key"), "missing.key");
        assert_eq!(t(Locale::SimplifiedChinese, "missing.key"), "missing.key");
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
