mod cdp;
mod i18n;
mod paths;
mod platform;
mod theme;

use anyhow::{Result, anyhow, bail};
use gpui::{
    AnyElement, App, Application, AssetSource, Bounds, ClipboardItem, Context, ObjectFit,
    SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, relative, rgb, rgba, size, svg,
};
#[cfg(target_os = "macos")]
use gpui::{MouseButton, point};
use gpui_component::{
    Root,
    input::{Input, InputState},
    tooltip::Tooltip,
};
use i18n::{Appearance, Language, Locale, t};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{borrow::Cow, env, fs, path::PathBuf};

struct CodeFaceAssets;

impl AssetSource for CodeFaceAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/refresh-cw.svg" => include_bytes!("../../resources/icons/refresh-cw.svg"),
            "icons/plus.svg" => include_bytes!("../../resources/icons/plus.svg"),
            "icons/settings.svg" => include_bytes!("../../resources/icons/settings.svg"),
            "icons/pencil.svg" => include_bytes!("../../resources/icons/pencil.svg"),
            "icons/trash-2.svg" => include_bytes!("../../resources/icons/trash-2.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path == "icons" {
            Ok([
                "refresh-cw.svg",
                "plus.svg",
                "settings.svg",
                "pencil.svg",
                "trash-2.svg",
            ]
            .into_iter()
            .map(SharedString::from)
            .collect())
        } else {
            Ok(Vec::new())
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ThemeSummary {
    id: String,
    name: String,
    description: String,
    image: PathBuf,
    is_system: bool,
    is_market: bool,
    background: u32,
    panel: u32,
    panel_alt: u32,
    accent: u32,
    accent_alt: u32,
    text: u32,
    muted: u32,
    line: u32,
    background_position: (f32, f32),
    sidebar_width: f32,
    content_max_width: f32,
    composer_max_width: f32,
    brand: bool,
    avatar: Option<PathBuf>,
    has_background_image: bool,
}

const SYSTEM_THEME_ID: &str = "__codeface-system-theme__";
const PREVIEW_PROJECT_PANEL_MIN_HEIGHT: f32 = 64.;
const PREVIEW_PROJECT_COMPOSER_GAP: f32 = 10.;
const _: () = assert!(PREVIEW_PROJECT_PANEL_MIN_HEIGHT >= 60.);
const _: () = assert!(PREVIEW_PROJECT_COMPOSER_GAP >= 8.);

#[derive(Clone, Copy)]
struct AppPalette {
    background: u32,
    surface: u32,
    surface_hover: u32,
    border: u32,
    text: u32,
    muted: u32,
    accent: u32,
    accent_soft: u32,
}

impl Appearance {
    fn palette(self) -> AppPalette {
        match self {
            Self::Light => AppPalette {
                background: 0xF4F5F8,
                surface: 0xFFFFFF,
                surface_hover: 0xECEEF4,
                border: 0xD9DCE5,
                text: 0x20232B,
                muted: 0x666D7D,
                accent: 0x6D4AFF,
                accent_soft: 0xEEEAFE,
            },
            Self::Dark => AppPalette {
                background: 0x10121A,
                surface: 0x191C27,
                surface_hover: 0x252938,
                border: 0x2A2E3D,
                text: 0xDCE1ED,
                muted: 0x8B93A7,
                accent: 0x9B87FF,
                accent_soft: 0x292440,
            },
        }
    }
}

struct CodeFaceApp {
    themes: Vec<ThemeSummary>,
    selected: Option<String>,
    applied: Option<String>,
    status: SharedString,
    busy: bool,
    settings_open: bool,
    codex_menu_open: bool,
    add_theme_menu_open: bool,
    language: Language,
    appearance: Appearance,
    pending_delete: Option<String>,
    codexthemes_open: bool,
    codexthemes_error: Option<SharedString>,
    market_results: Vec<theme::MarketTheme>,
    editing_source: bool,
    editing_id: Option<String>,
    draft_image: Option<PathBuf>,
    theme_json_editor: gpui::Entity<InputState>,
    css_editor: gpui::Entity<InputState>,
    codexthemes_input: gpui::Entity<InputState>,
}

fn apply_theme_checked(
    id: &str,
    restart_existing: bool,
) -> Result<(cdp::RuntimeState, cdp::HealthReport)> {
    let previous = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<cdp::RuntimeState>(&text).ok());
    let active = theme::activate(id)?;
    let manifest: Value = serde_json::from_str(&fs::read_to_string(active.join("theme.json"))?)?;
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_owned();
    let state = cdp::apply_active(name, restart_existing)?;
    let report = cdp::health_check(state.port, id)?;
    if report.healthy {
        return Ok((state, report));
    }

    let previous_id = previous
        .as_ref()
        .map(|state| state.theme_id.as_str())
        .filter(|previous_id| !previous_id.is_empty())
        .map(str::to_owned);
    let rollback_id = if previous_id.as_deref() == Some(id) {
        theme::rollback_theme(id).ok().map(|_| id.to_owned())
    } else {
        previous_id
    };
    if let Some(previous_id) = rollback_id {
        let previous_active = theme::activate(&previous_id)?;
        let previous_manifest: Value =
            serde_json::from_str(&fs::read_to_string(previous_active.join("theme.json"))?)?;
        let previous_name = previous_manifest
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&previous_id)
            .to_owned();
        cdp::apply_active(previous_name, false)?;
    } else {
        cdp::restore_native()?;
    }
    bail!(
        "theme health check failed and was rolled back: {}\n{}",
        if report.issues.is_empty() {
            "unknown runtime failure".to_owned()
        } else {
            report.issues.join("; ")
        },
        serde_json::to_string_pretty(&report)?
    )
}

fn install_codexthemes_checked(input: &str) -> Result<String> {
    let applied_id = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<cdp::RuntimeState>(&text).ok())
        .filter(|state| state.injection_enabled)
        .map(|state| state.theme_id);
    let id = theme::install_from_codexthemes(input)?;
    if applied_id.as_deref() == Some(&id) {
        apply_theme_checked(&id, false)?;
    }
    Ok(id)
}

fn rollback_theme_checked(id: &str) -> Result<theme::BackupInfo> {
    let is_applied = fs::read_to_string(paths::state_path()?)
        .ok()
        .and_then(|text| serde_json::from_str::<cdp::RuntimeState>(&text).ok())
        .is_some_and(|state| state.injection_enabled && state.theme_id == id);
    let backup = theme::rollback_theme(id)?;
    if is_applied {
        apply_theme_checked(id, false)?;
    }
    Ok(backup)
}

impl CodeFaceApp {
    fn error_message(locale: Locale, error: &anyhow::Error) -> SharedString {
        format!("{}: {error:#}", t(locale, "operation_failed")).into()
    }
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme_json_editor = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("json")
                .line_number(true)
                .default_value(theme::DEFAULT_JSON)
        });
        let css_editor = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("css")
                .line_number(true)
                .default_value(theme::DEFAULT_CSS)
        });
        let codexthemes_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("portal-panic or https://codexthemes.ai/themes/portal-panic")
        });
        let language = i18n::load();
        let appearance = i18n::load_appearance();
        let mut app = Self {
            themes: Vec::new(),
            selected: None,
            applied: None,
            status: t(language.effective(), "ready").into(),
            busy: false,
            settings_open: false,
            codex_menu_open: false,
            add_theme_menu_open: false,
            language,
            appearance,
            pending_delete: None,
            codexthemes_open: false,
            codexthemes_error: None,
            market_results: Vec::new(),
            editing_source: false,
            editing_id: None,
            draft_image: None,
            theme_json_editor,
            css_editor,
            codexthemes_input,
        };
        if let Err(error) = theme::install_bundled_themes() {
            app.status = Self::error_message(app.locale(), &error);
        }
        app.reload();
        app.applied = app.detect_applied_theme();
        app
    }

    fn locale(&self) -> Locale {
        self.language.effective()
    }

    fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        self.language = language;
        let status = match i18n::save(language, self.appearance) {
            Ok(()) => t(self.locale(), "settings_saved").into(),
            Err(error) => format!("{}: {error:#}", t(self.locale(), "operation_failed")).into(),
        };
        self.reload();
        self.status = status;
        cx.notify();
    }

    fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        self.appearance = appearance;
        self.status = match i18n::save(self.language, appearance) {
            Ok(()) => t(self.locale(), "settings_saved").into(),
            Err(error) => format!("{}: {error:#}", t(self.locale(), "operation_failed")).into(),
        };
        cx.notify();
    }

    fn state_root() -> PathBuf {
        paths::state_root().unwrap_or_else(|_| PathBuf::from("."))
    }

    fn preview_color(value: Option<&Value>, fallback: u32) -> u32 {
        value
            .and_then(Value::as_str)
            .and_then(|color| color.strip_prefix('#'))
            .filter(|color| color.len() == 6)
            .and_then(|color| u32::from_str_radix(color, 16).ok())
            .unwrap_or(fallback)
    }

    fn preview_number(value: Option<&Value>, fallback: f32) -> f32 {
        value
            .and_then(Value::as_f64)
            .map(|number| number as f32)
            .filter(|number| number.is_finite())
            .unwrap_or(fallback)
    }

    fn preview_position(value: Option<&Value>) -> (f32, f32) {
        fn component(value: Option<&str>, fallback: f32) -> f32 {
            match value.unwrap_or_default() {
                "left" | "top" => 0.,
                "center" => 0.5,
                "right" | "bottom" => 1.,
                percentage => percentage
                    .strip_suffix('%')
                    .and_then(|number| number.parse::<f32>().ok())
                    .map(|number| (number / 100.).clamp(0., 1.))
                    .unwrap_or(fallback),
            }
        }

        let mut parts = value
            .and_then(Value::as_str)
            .unwrap_or("center center")
            .split_whitespace();
        (component(parts.next(), 0.5), component(parts.next(), 0.5))
    }

    fn detect_applied_theme(&self) -> Option<String> {
        let state = fs::read_to_string(paths::state_path().ok()?).ok()?;
        let state: cdp::RuntimeState = serde_json::from_str(&state).ok()?;
        if !state.injection_enabled {
            return None;
        }
        self.themes
            .iter()
            .find(|theme| !theme.is_system && theme.name == state.theme_name)
            .map(|theme| theme.id.clone())
    }

    fn run_operation_async<T: Send + 'static>(
        &mut self,
        label: &'static str,
        operation: impl FnOnce() -> Result<T> + Send + 'static,
        on_done: impl FnOnce(&mut Self, Result<T>) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            self.status = t(self.locale(), "busy").into();
            cx.notify();
            return;
        }
        self.busy = true;
        self.status = match self.locale() {
            Locale::SimplifiedChinese => format!("正在{label}…").into(),
            Locale::English => format!("{label}…").into(),
        };
        cx.notify();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |entity, cx| {
            let result = executor.spawn(async move { operation() }).await;
            let _ = entity.update(cx, move |app, cx| {
                app.busy = false;
                on_done(app, result);
                cx.notify();
            });
        })
        .detach();
    }

    fn reload(&mut self) {
        self.themes.clear();
        let root = Self::state_root().join("themes");
        for entry in fs::read_dir(&root).into_iter().flatten().flatten() {
            let dir = entry.path();
            let config = dir.join("theme.json");
            let Ok(data) = fs::read_to_string(config) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            let Some(id) = dir.file_name().and_then(|v| v.to_str()).map(str::to_owned) else {
                continue;
            };
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_owned();
            let description = value
                .get("description")
                .or_else(|| value.get("tagline"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| t(self.locale(), "custom_theme"))
                .to_owned();
            let image_name = value
                .get("image")
                .and_then(Value::as_str)
                .unwrap_or("background.jpg");
            let colors = value.get("colors");
            let layout = value.get("layout");
            let image = dir.join(image_name);
            let has_background_image = image::image_dimensions(&image)
                .is_ok_and(|(width, height)| width > 1 || height > 1);
            let brand = value
                .get("chrome")
                .and_then(|chrome| chrome.get("brand"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_market = value.get("codexthemes").is_some();
            let avatar = dir.join("avatar.png");
            self.themes.push(ThemeSummary {
                id,
                name,
                description,
                image,
                is_system: false,
                is_market,
                background: Self::preview_color(colors.and_then(|v| v.get("background")), 0xFFFFFF),
                panel: Self::preview_color(colors.and_then(|v| v.get("panel")), 0xFFFFFF),
                panel_alt: Self::preview_color(colors.and_then(|v| v.get("panelAlt")), 0xF3F3F4),
                accent: Self::preview_color(colors.and_then(|v| v.get("accent")), 0x7C3AED),
                accent_alt: Self::preview_color(colors.and_then(|v| v.get("accentAlt")), 0x9B87FF),
                text: Self::preview_color(colors.and_then(|v| v.get("text")), 0x222222),
                muted: Self::preview_color(colors.and_then(|v| v.get("muted")), 0x777777),
                line: Self::preview_color(colors.and_then(|v| v.get("line")), 0xD7D7DB),
                background_position: Self::preview_position(
                    layout.and_then(|v| v.get("backgroundPosition")),
                ),
                sidebar_width: Self::preview_number(
                    layout.and_then(|v| v.get("sidebarWidth")),
                    272.,
                )
                .clamp(180., 440.),
                content_max_width: Self::preview_number(
                    layout.and_then(|v| v.get("contentMaxWidth")),
                    980.,
                )
                .clamp(640., 1600.),
                composer_max_width: Self::preview_number(
                    layout.and_then(|v| v.get("composerMaxWidth")),
                    840.,
                )
                .clamp(520., 1400.),
                brand,
                avatar: avatar.exists().then_some(avatar),
                has_background_image,
            });
        }
        self.themes.sort_by_key(|theme| theme.name.to_lowercase());
        self.themes.insert(
            0,
            ThemeSummary {
                id: SYSTEM_THEME_ID.into(),
                name: t(self.locale(), "system_theme").into(),
                description: t(self.locale(), "system_theme_badge").into(),
                image: PathBuf::new(),
                is_system: true,
                is_market: false,
                background: 0xFFFFFF,
                panel: 0xF7F7F8,
                panel_alt: 0xECECEE,
                accent: 0xD8D8DC,
                accent_alt: 0xE4E4E7,
                text: 0x202123,
                muted: 0x6E6E73,
                line: 0xD7D7DB,
                background_position: (0.5, 0.5),
                sidebar_width: 272.,
                content_max_width: 980.,
                composer_max_width: 840.,
                brand: false,
                avatar: None,
                has_background_image: false,
            },
        );
        if self
            .selected
            .as_ref()
            .is_none_or(|id| !self.themes.iter().any(|t| &t.id == id))
        {
            self.selected = self.themes.first().map(|theme| theme.id.clone());
        }
        self.status = match self.locale() {
            Locale::SimplifiedChinese => {
                format!("已载入 {} 个自定义主题", self.themes.len() - 1).into()
            }
            Locale::English => format!("Loaded {} custom themes", self.themes.len() - 1).into(),
        };
    }

    fn refresh_themes(&mut self, cx: &mut Context<Self>) {
        self.reload();
        self.applied = self.detect_applied_theme();
        self.codex_menu_open = false;
        self.add_theme_menu_open = false;
        self.pending_delete = None;
        cx.notify();
    }

    fn set_editor_text(
        editor: &gpui::Entity<InputState>,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        editor.update(cx, |editor, cx| editor.set_value(value, window, cx));
    }

    fn begin_new_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let json = match theme::new_theme_json() {
            Ok(json) => json,
            Err(error) => {
                self.status = Self::error_message(self.locale(), &error);
                cx.notify();
                return;
            }
        };
        self.editing_source = true;
        self.add_theme_menu_open = false;
        self.settings_open = false;
        self.editing_id = None;
        self.draft_image = None;
        Self::set_editor_text(&self.theme_json_editor, json, window, cx);
        Self::set_editor_text(&self.css_editor, theme::DEFAULT_CSS, window, cx);
        self.status = t(self.locale(), "new_source_ready").into();
        cx.notify();
    }

    fn begin_edit_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = match self.selected_id() {
            Ok(id) => id,
            Err(error) => {
                self.status = Self::error_message(self.locale(), &error);
                cx.notify();
                return;
            }
        };
        if id == SYSTEM_THEME_ID {
            self.status = t(self.locale(), "system_theme_locked").into();
            cx.notify();
            return;
        }
        let result = theme::read_source(&id);
        match result {
            Ok((json, css, image)) => {
                Self::set_editor_text(&self.theme_json_editor, json, window, cx);
                Self::set_editor_text(&self.css_editor, css, window, cx);
                self.editing_source = true;
                self.settings_open = false;
                self.editing_id = Some(id);
                self.draft_image = Some(image);
                self.status = t(self.locale(), "source_loaded").into();
            }
            Err(error) => self.status = Self::error_message(self.locale(), &error),
        }
        cx.notify();
    }

    fn choose_source_image(&mut self, cx: &mut Context<Self>) {
        let title = t(self.locale(), "select_image_dialog").to_owned();
        let filter_name = t(self.locale(), "image_filter").to_owned();
        cx.spawn(async move |entity, cx| {
            let image = theme::choose_image(title, filter_name).await;
            if let Some(image) = image {
                let _ = entity.update(cx, |app, cx| {
                    app.draft_image = Some(image.clone());
                    app.status =
                        format!("{}: {}", t(app.locale(), "image_selected"), image.display())
                            .into();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn begin_save_source(&mut self, apply: bool, cx: &mut Context<Self>) {
        if self.busy {
            self.status = t(self.locale(), "busy").into();
            cx.notify();
            return;
        }
        let image = self.draft_image.clone();
        let json = self.theme_json_editor.read(cx).value().to_string();
        let css = self.css_editor.read(cx).value().to_string();
        let existing_id = self.editing_id.clone();
        let locale = self.locale();
        self.run_operation_async(
            if apply {
                t(locale, "save_apply")
            } else {
                t(locale, "save")
            },
            move || {
                let id = theme::save(&json, &css, image.as_deref(), existing_id.as_deref())?;
                if apply {
                    apply_theme_checked(&id, false)?;
                }
                Ok(id)
            },
            move |app, result| match result {
                Ok(id) => {
                    app.reload();
                    app.selected = Some(id.clone());
                    if apply {
                        app.applied = Some(id.clone());
                    }
                    app.editing_id = Some(id);
                    app.status =
                        t(app.locale(), if apply { "saved_applied" } else { "saved" }).into();
                }
                Err(error) => app.status = Self::error_message(app.locale(), &error),
            },
            cx,
        );
    }

    fn begin_import_pack(&mut self, cx: &mut Context<Self>) {
        self.add_theme_menu_open = false;
        let title = t(self.locale(), "select_pack_dialog").to_owned();
        cx.spawn(async move |entity, cx| {
            let directory = theme::choose_pack(title).await;
            if let Some(directory) = directory {
                let _ = entity.update(cx, |app, cx| {
                    let locale = app.locale();
                    app.run_operation_async(
                        t(locale, "import_pack"),
                        move || theme::import_directory(&directory),
                        move |app, result| match result {
                            Ok(id) => {
                                app.reload();
                                app.selected = Some(id);
                                app.status = t(app.locale(), "pack_imported").into();
                            }
                            Err(error) => app.status = Self::error_message(app.locale(), &error),
                        },
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    fn begin_install_codexthemes(&mut self, cx: &mut Context<Self>) {
        let input = self.codexthemes_input.read(cx).value().to_string();
        self.begin_install_codexthemes_value(input, cx);
    }

    fn begin_install_codexthemes_value(&mut self, input: String, cx: &mut Context<Self>) {
        let locale = self.locale();
        self.codexthemes_error = None;
        self.run_operation_async(
            t(locale, "install_codexthemes"),
            move || install_codexthemes_checked(&input),
            move |app, result| match result {
                Ok(id) => {
                    app.reload();
                    app.selected = Some(id);
                    app.codexthemes_open = false;
                    app.status = t(app.locale(), "codexthemes_installed").into();
                }
                Err(error) => {
                    let message = Self::error_message(app.locale(), &error);
                    app.status = message.clone();
                    app.codexthemes_error = Some(message);
                }
            },
            cx,
        );
    }

    fn begin_search_codexthemes(&mut self, cx: &mut Context<Self>) {
        let query = self.codexthemes_input.read(cx).value().to_string();
        let locale = self.locale();
        self.codexthemes_error = None;
        self.run_operation_async(
            t(locale, "search_codexthemes"),
            move || theme::search_codexthemes(&query),
            |app, result| match result {
                Ok(results) => {
                    app.market_results = results;
                    app.status = t(app.locale(), "market_results_loaded").into();
                }
                Err(error) => {
                    let message = Self::error_message(app.locale(), &error);
                    app.status = message.clone();
                    app.codexthemes_error = Some(message);
                }
            },
            cx,
        );
    }

    fn begin_check_theme_update(&mut self, cx: &mut Context<Self>) {
        let id = match self.selected_id() {
            Ok(id) if id != SYSTEM_THEME_ID => id,
            _ => return,
        };
        let locale = self.locale();
        self.run_operation_async(
            t(locale, "check_updates"),
            move || theme::market_update_available(&id),
            |app, result| {
                app.status = match result {
                    Ok(true) => t(app.locale(), "update_available").into(),
                    Ok(false) => t(app.locale(), "up_to_date").into(),
                    Err(error) => Self::error_message(app.locale(), &error),
                }
            },
            cx,
        );
    }

    fn begin_rollback_theme(&mut self, cx: &mut Context<Self>) {
        let id = match self.selected_id() {
            Ok(id) if id != SYSTEM_THEME_ID => id,
            _ => return,
        };
        let selected = id.clone();
        let locale = self.locale();
        self.run_operation_async(
            t(locale, "rollback_theme"),
            move || rollback_theme_checked(&id),
            move |app, result| match result {
                Ok(_) => {
                    app.reload();
                    app.selected = Some(selected);
                    app.status = t(app.locale(), "theme_rolled_back").into();
                }
                Err(error) => app.status = Self::error_message(app.locale(), &error),
            },
            cx,
        );
    }

    fn begin_export_theme(&mut self, cx: &mut Context<Self>) {
        let id = match self.selected_id() {
            Ok(id) if id != SYSTEM_THEME_ID => id,
            _ => return,
        };
        let locale = self.locale();
        self.run_operation_async(
            t(locale, "export_theme"),
            move || theme::export_theme(&id),
            |app, result| {
                app.status = match result {
                    Ok(path) => {
                        format!("{}: {}", t(app.locale(), "theme_exported"), path.display()).into()
                    }
                    Err(error) => Self::error_message(app.locale(), &error),
                }
            },
            cx,
        );
    }

    fn selected_id(&self) -> Result<String> {
        self.selected
            .clone()
            .ok_or_else(|| anyhow!("Select a theme first"))
    }

    fn begin_switch(&mut self, cx: &mut Context<Self>) {
        let id = match self.selected_id() {
            Ok(id) => id,
            Err(error) => {
                self.status = Self::error_message(self.locale(), &error);
                cx.notify();
                return;
            }
        };
        let locale = self.locale();
        if id == SYSTEM_THEME_ID {
            self.run_operation_async(
                t(locale, "apply_theme"),
                cdp::restore_native,
                |app, result| {
                    app.status = match result {
                        Ok(()) => {
                            app.applied = None;
                            t(app.locale(), "system_theme_applied").into()
                        }
                        Err(error) => Self::error_message(app.locale(), &error),
                    }
                },
                cx,
            );
            return;
        }
        let applied_id = id.clone();
        self.run_operation_async(
            t(locale, "apply_theme"),
            move || apply_theme_checked(&id, false).map(|(state, _)| state),
            move |app, result| match result {
                Ok(state) => {
                    app.applied = Some(applied_id.clone());
                    app.status = format!(
                        "{} {} · CDP {}",
                        t(app.locale(), "applied"),
                        state.theme_name,
                        state.port
                    )
                    .into()
                }
                Err(error) => app.status = Self::error_message(app.locale(), &error),
            },
            cx,
        );
    }

    fn begin_close_codex(&mut self, cx: &mut Context<Self>) {
        let locale = self.locale();
        self.run_operation_async(
            t(locale, "close_codex"),
            || {
                cdp::remove_live_skin()?;
                cdp::close_codex()
            },
            |app, result| {
                app.status = match result {
                    Ok(()) => {
                        app.applied = None;
                        t(app.locale(), "codex_closed").into()
                    }
                    Err(error) => Self::error_message(app.locale(), &error),
                }
            },
            cx,
        );
    }

    fn begin_restart_codex(&mut self, cx: &mut Context<Self>) {
        let locale = self.locale();
        self.run_operation_async(
            t(locale, "restart_codex"),
            || {
                cdp::remove_live_skin()?;
                cdp::restart_codex()
            },
            |app, result| {
                app.status = match result {
                    Ok(()) => {
                        app.applied = None;
                        t(app.locale(), "codex_restarted").into()
                    }
                    Err(error) => Self::error_message(app.locale(), &error),
                }
            },
            cx,
        );
    }

    fn copy_context_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editing_id.as_deref() else {
            if self.busy {
                self.status = t(self.locale(), "busy").into();
                cx.notify();
                return;
            }

            let image = self.draft_image.clone();
            let json = self.theme_json_editor.read(cx).value().to_string();
            let css = self.css_editor.read(cx).value().to_string();
            let chinese = self.locale() == Locale::SimplifiedChinese;
            self.busy = true;
            self.status = match self.locale() {
                Locale::SimplifiedChinese => format!("正在{}…", t(self.locale(), "save")).into(),
                Locale::English => format!("{}…", t(self.locale(), "save")).into(),
            };
            cx.notify();

            let executor = cx.background_executor().clone();
            cx.spawn(async move |entity, cx| {
                let result = executor
                    .spawn(async move {
                        let id = theme::save(&json, &css, image.as_deref(), None)?;
                        let prompt = theme::context_prompt(&id, chinese)?;
                        Ok::<_, anyhow::Error>((id, prompt))
                    })
                    .await;
                let _ = entity.update(cx, move |app, cx| {
                    app.busy = false;
                    match result {
                        Ok((id, prompt)) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(prompt));
                            app.reload();
                            app.selected = Some(id.clone());
                            app.editing_id = Some(id);
                            app.status = t(app.locale(), "prompt_copied").into();
                        }
                        Err(error) => {
                            app.status = Self::error_message(app.locale(), &error);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
            return;
        };
        match theme::context_prompt(id, self.locale() == Locale::SimplifiedChinese) {
            Ok(prompt) => {
                cx.write_to_clipboard(ClipboardItem::new_string(prompt));
                self.status = t(self.locale(), "prompt_copied").into();
            }
            Err(error) => self.status = Self::error_message(self.locale(), &error),
        }
        cx.notify();
    }

    fn confirm_delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.pending_delete.take() else {
            return;
        };
        if id == SYSTEM_THEME_ID {
            self.status = t(self.locale(), "system_theme_locked").into();
            cx.notify();
            return;
        }
        let result = if self.applied.as_deref() == Some(&id) {
            cdp::restore_native().and_then(|()| theme::delete(&id))
        } else {
            theme::delete(&id)
        };
        match result {
            Ok(()) => {
                if self.applied.as_deref() == Some(&id) {
                    self.applied = Some(SYSTEM_THEME_ID.to_owned());
                }
                if self.selected.as_deref() == Some(&id) {
                    self.selected = None;
                }
                if self.editing_id.as_deref() == Some(&id) {
                    self.editing_id = None;
                    self.editing_source = false;
                }
                self.reload();
                self.status = t(self.locale(), "deleted").into();
            }
            Err(error) => self.status = Self::error_message(self.locale(), &error),
        }
        cx.notify();
    }
}

fn button(
    cx: &Context<CodeFaceApp>,
    label: impl Into<SharedString>,
    primary: bool,
    handler: impl Fn(&mut CodeFaceApp, &mut Window, &mut Context<CodeFaceApp>) + 'static,
) -> AnyElement {
    let label = label.into();
    button_with_id(cx, label.clone(), label, primary, handler)
}

fn button_with_id(
    cx: &Context<CodeFaceApp>,
    id: SharedString,
    label: impl Into<SharedString>,
    primary: bool,
    handler: impl Fn(&mut CodeFaceApp, &mut Window, &mut Context<CodeFaceApp>) + 'static,
) -> AnyElement {
    let label = label.into();
    div()
        .id(id)
        .px_4()
        .py_2()
        .rounded_lg()
        .cursor_pointer()
        .bg(if primary {
            rgb(0x7C3AED)
        } else {
            rgb(0x252938)
        })
        .text_color(rgb(0xF8FAFC))
        .hover(|item| item.opacity(0.82))
        .child(label)
        .on_click(cx.listener(move |app, _, window, cx| handler(app, window, cx)))
        .into_any_element()
}

fn icon_button(
    cx: &Context<CodeFaceApp>,
    id: &'static str,
    icon_path: &'static str,
    label: &'static str,
    handler: impl Fn(&mut CodeFaceApp, &mut Window, &mut Context<CodeFaceApp>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .size(px(34.))
        .rounded_lg()
        .cursor_pointer()
        .bg(rgb(0x252938))
        .text_color(rgb(0xD8DCE7))
        .flex()
        .items_center()
        .justify_center()
        .hover(|item| item.opacity(0.82))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
        .child(
            svg()
                .path(icon_path)
                .size(px(17.))
                .text_color(rgb(0xD8DCE7)),
        )
        .on_click(cx.listener(move |app, _, window, cx| handler(app, window, cx)))
        .into_any_element()
}

fn card_action_button(
    cx: &Context<CodeFaceApp>,
    id: SharedString,
    icon_path: &'static str,
    label: &'static str,
    danger: bool,
    handler: impl Fn(&mut CodeFaceApp, &mut Window, &mut Context<CodeFaceApp>) + 'static,
) -> AnyElement {
    let icon_color = if danger { 0xF3A6AE } else { 0xD8DCE7 };
    div()
        .id(id)
        .size(px(30.))
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(0x252938))
        .text_color(rgb(icon_color))
        .flex()
        .items_center()
        .justify_center()
        .hover(|item| item.opacity(0.84))
        .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
        .on_click(cx.listener(move |app, _, window, cx| {
            cx.stop_propagation();
            handler(app, window, cx);
        }))
        .child(
            svg()
                .path(icon_path)
                .size(px(15.))
                .text_color(rgb(icon_color)),
        )
        .into_any_element()
}

fn preview_nav_item(label: SharedString, selected: bool, theme: &ThemeSummary) -> AnyElement {
    div()
        .h(px(28.))
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .when(selected, |item| {
            item.bg(rgba((theme.panel_alt << 8) | 0xD8))
        })
        .child(div().size(px(6.)).rounded_full().bg(if selected {
            rgb(theme.accent)
        } else {
            rgb(theme.muted)
        }))
        .child(label)
        .into_any_element()
}

fn preview_workspace(theme: ThemeSummary, locale: Locale) -> AnyElement {
    let titlebar_height = 24.;
    let sidebar_width = (theme.sidebar_width * 0.55).clamp(138., 190.);
    let composer_fraction =
        ((theme.composer_max_width / theme.content_max_width) * 0.84).clamp(0.62, 0.78);
    let project_fraction = (composer_fraction + 0.06).min(0.9);
    let sidebar_background = rgba((theme.panel << 8) | if theme.is_system { 0xFA } else { 0xD8 });
    let sidebar_fade = rgba((theme.panel << 8) | if theme.is_system { 0xF0 } else { 0xC8 });
    let transparent_panel = rgba(theme.panel << 8);
    let avatar = theme.avatar.clone();

    let title_brand: AnyElement = if theme.brand {
        div()
            .absolute()
            .top_0()
            .left(px(sidebar_width + 46.))
            .h(px(titlebar_height))
            .flex()
            .items_center()
            .gap_2()
            .children(avatar.clone().map(|path| {
                img(path)
                    .size(px(22.))
                    .rounded_md()
                    .object_fit(ObjectFit::Cover)
            }))
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(theme.accent))
                    .child(theme.name.clone()),
            )
            .into_any_element()
    } else {
        div().into_any_element()
    };

    let sidebar_header = div()
        .h(px(38.))
        .px_2()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_base()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Codex"),
        )
        .child(
            div()
                .size(px(12.))
                .rounded_full()
                .border_1()
                .border_color(rgb(theme.muted)),
        );

    let background: AnyElement = if theme.has_background_image {
        let overscan = 0.1;
        img(theme.image.clone())
            .absolute()
            .top(relative(-theme.background_position.1 * overscan))
            .left(relative(-theme.background_position.0 * overscan))
            .w(relative(1. + overscan))
            .h(relative(1. + overscan))
            .object_fit(ObjectFit::Cover)
            .into_any_element()
    } else {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(linear_gradient(
                if theme.brand { 180. } else { 145. },
                linear_color_stop(
                    rgb(if theme.brand {
                        theme.accent_alt
                    } else {
                        theme.panel_alt
                    }),
                    0.,
                ),
                linear_color_stop(
                    rgb(if theme.brand {
                        theme.panel
                    } else {
                        theme.background
                    }),
                    1.,
                ),
            ))
            .into_any_element()
    };

    let watermark: AnyElement = if theme.brand {
        avatar
            .map(|path| {
                img(path)
                    .absolute()
                    .top(relative(0.18))
                    .right(relative(0.08))
                    .size(px(124.))
                    .object_fit(ObjectFit::Contain)
                    .opacity(0.14)
                    .into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element())
    } else {
        div().into_any_element()
    };

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .rounded_xl()
        .border_1()
        .border_color(rgb(theme.line))
        .bg(rgb(theme.background))
        .text_color(rgb(theme.text))
        .child(background)
        .child(watermark)
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(titlebar_height))
                .px_3()
                .bg(rgba((theme.panel << 8) | 0xD2))
                .border_b_1()
                .border_color(rgba((theme.line << 8) | 0x80))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .text_color(rgb(theme.muted))
                        .child(
                            div()
                                .w(px(14.))
                                .h(px(10.))
                                .rounded_sm()
                                .border_1()
                                .border_color(rgb(theme.muted)),
                        )
                        .child("‹")
                        .child("›"),
                )
                .child(div().flex().gap_3().children((0..2).map(|_| {
                    div()
                        .size(px(10.))
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(theme.muted))
                })))
                .child(title_brand),
        )
        .child(
            div()
                .absolute()
                .top(px(titlebar_height))
                .left_0()
                .bottom_0()
                .w(px(sidebar_width))
                .px_3()
                .py_3()
                .bg(sidebar_background)
                .flex()
                .flex_col()
                .text_xs()
                .child(sidebar_header)
                .child(preview_nav_item(t(locale, "new_task").into(), true, &theme))
                .child(preview_nav_item(
                    t(locale, "scheduled").into(),
                    false,
                    &theme,
                ))
                .child(preview_nav_item(t(locale, "plugins").into(), false, &theme))
                .child(preview_nav_item(
                    t(locale, "pull_requests").into(),
                    false,
                    &theme,
                ))
                .child(
                    div()
                        .mt_4()
                        .px_2()
                        .pb_1()
                        .text_color(rgb(theme.muted))
                        .child(t(locale, "projects")),
                )
                .child(div().px_2().py_1().truncate().child(theme.name.clone()))
                .child(
                    div()
                        .px_4()
                        .py_1()
                        .truncate()
                        .text_color(rgb(theme.muted))
                        .child("main"),
                )
                .child(
                    div()
                        .px_4()
                        .py_1()
                        .truncate()
                        .child(t(locale, "theme_preview")),
                )
                .child(
                    div()
                        .mt_2()
                        .px_2()
                        .py_1()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child("CodeFace"),
                )
                .child(
                    div()
                        .px_4()
                        .py_1()
                        .truncate()
                        .text_color(rgb(theme.muted))
                        .child(t(locale, "custom_theme")),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .h(px(30.))
                        .px_2()
                        .border_t_1()
                        .border_color(rgba((theme.line << 8) | 0xA0))
                        .flex()
                        .items_center()
                        .text_color(rgb(theme.muted))
                        .child(t(locale, "settings")),
                ),
        )
        .child(
            div()
                .absolute()
                .top(px(titlebar_height))
                .left(px(sidebar_width))
                .bottom_0()
                .w(px(52.))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(sidebar_fade, 0.),
                    linear_color_stop(transparent_panel, 1.),
                )),
        )
        .child(
            div()
                .absolute()
                .top(px(titlebar_height))
                .left(px(sidebar_width))
                .right_0()
                .bottom_0()
                .px_5()
                .py_4()
                .flex()
                .flex_col()
                .child(div().flex_1())
                .child(
                    div()
                        .mx_auto()
                        .mb(px(PREVIEW_PROJECT_COMPOSER_GAP))
                        .w(relative(project_fraction))
                        .min_h(px(PREVIEW_PROJECT_PANEL_MIN_HEIGHT))
                        .px_3()
                        .py_2()
                        .rounded_xl()
                        .bg(rgba((theme.panel << 8) | 0xE8))
                        .border_1()
                        .border_color(rgba((theme.line << 8) | 0xB8))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_xs()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(rgb(theme.accent))
                                .child(t(locale, "preview_select_project")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgba((theme.line << 8) | 0xD8))
                                        .child("CodeFace"),
                                )
                                .child(
                                    div()
                                        .text_color(rgb(theme.muted))
                                        .child(t(locale, "preview_local")),
                                )
                                .child(div().text_color(rgb(theme.muted)).child("main")),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .mx_auto()
                        .mb_3()
                        .w(relative(composer_fraction))
                        .min_h(px(76.))
                        .rounded_xl()
                        .bg(rgba((theme.panel << 8) | 0xEC))
                        .border_1()
                        .border_color(rgba((theme.line << 8) | 0xE0))
                        .px_4()
                        .py_3()
                        .flex()
                        .flex_col()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(theme.muted))
                                .child(t(locale, "composer")),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .text_color(rgb(theme.muted))
                                        .child("+")
                                        .child("@"),
                                )
                                .child(
                                    div()
                                        .size(px(27.))
                                        .rounded_lg()
                                        .bg(rgb(theme.accent))
                                        .text_color(rgb(theme.panel))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child("↑"),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

#[cfg(target_os = "macos")]
fn macos_titlebar() -> AnyElement {
    let control = |id: &'static str, color: u32, action: fn(&mut Window)| {
        div()
            .id(id)
            .size(px(12.))
            .rounded_full()
            .bg(rgb(color))
            .cursor_pointer()
            .hover(|item| item.opacity(0.84))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            })
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                action(window);
            })
    };
    div()
        .h(px(38.))
        .px_3()
        .bg(rgb(0x171922))
        .flex()
        .items_center()
        .gap_2()
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .child(control("window-close", 0xFF5F57, |window| {
            window.remove_window()
        }))
        .child(control("window-minimize", 0xFEBC2E, |window| {
            window.minimize_window()
        }))
        .child(control("window-zoom", 0x28C840, |window| {
            window.zoom_window()
        }))
        .into_any_element()
}

#[cfg(not(target_os = "macos"))]
fn macos_titlebar() -> AnyElement {
    div().into_any_element()
}

#[cfg(target_os = "macos")]
fn titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("CodeFace".into()),
        appears_transparent: true,
        traffic_light_position: Some(point(px(-100.), px(-100.))),
    }
}

#[cfg(not(target_os = "macos"))]
fn titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("CodeFace".into()),
        ..Default::default()
    }
}

impl Render for CodeFaceApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.locale();
        let palette = self.appearance.palette();
        let codex_menu_open = self.codex_menu_open;
        let add_theme_menu_open = self.add_theme_menu_open;
        let show_apply_bar = !self.settings_open && !self.editing_source;
        let selection_is_applied = self.selected == self.applied
            || (self.selected.as_deref() == Some(SYSTEM_THEME_ID) && self.applied.is_none());
        let selected = self.selected.clone();
        let selected_theme = self
            .themes
            .iter()
            .find(|theme| selected.as_ref() == Some(&theme.id))
            .cloned();
        let selected_is_market = selected_theme.as_ref().is_some_and(|theme| theme.is_market);
        let selected_is_custom = selected_theme
            .as_ref()
            .is_some_and(|theme| !theme.is_system);
        let selected_name = selected_theme
            .as_ref()
            .map(|theme| theme.name.clone())
            .unwrap_or_else(|| t(locale, "no_theme_selected").into());
        let applied_name = self
            .applied
            .as_ref()
            .and_then(|id| self.themes.iter().find(|theme| &theme.id == id))
            .map(|theme| theme.name.clone())
            .unwrap_or_else(|| t(locale, "system_theme").into());
        let pending_delete_name = self.pending_delete.as_ref().map(|id| {
            self.themes
                .iter()
                .find(|theme| &theme.id == id)
                .map(|theme| theme.name.clone())
                .unwrap_or_else(|| id.clone())
        });
        let cards: Vec<AnyElement> = self
            .themes
            .clone()
            .into_iter()
            .map(|theme| {
                let id = theme.id.clone();
                let is_system = theme.is_system;
                let edit_id = id.clone();
                let delete_id = id.clone();
                let active = selected.as_ref() == Some(&id);
                let applied = self.applied.as_ref() == Some(&id);
                let theme_colors = [theme.background, theme.panel, theme.accent];
                let status_label = if applied {
                    t(locale, "currently_applied")
                } else if is_system {
                    t(locale, "system_theme_badge_short")
                } else if active {
                    t(locale, "selected_for_preview")
                } else {
                    t(locale, "custom_theme")
                };
                let visual: AnyElement = if is_system {
                    div()
                        .w_full()
                        .h(px(118.))
                        .rounded_lg()
                        .bg(rgb(0xF6F6F7))
                        .border_1()
                        .border_color(rgb(0xD7D7DB))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(0x202123))
                        .text_2xl()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Aa")
                        .into_any_element()
                } else if theme.has_background_image {
                    img(theme.image)
                        .w_full()
                        .h(px(118.))
                        .rounded_lg()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                } else {
                    div()
                        .w_full()
                        .h(px(118.))
                        .rounded_lg()
                        .overflow_hidden()
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(rgb(theme.accent_alt), 0.),
                            linear_color_stop(rgb(theme.panel), 1.),
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(theme.avatar.clone().map(|path| {
                            img(path)
                                .size(px(68.))
                                .object_fit(ObjectFit::Contain)
                                .opacity(0.68)
                        }))
                        .into_any_element()
                };
                let actions: AnyElement = if !is_system {
                    div()
                        .flex()
                        .gap_1()
                        .child(card_action_button(
                            cx,
                            format!("theme-edit-{id}").into(),
                            "icons/pencil.svg",
                            t(locale, "edit"),
                            false,
                            move |app, window, cx| {
                                app.selected = Some(edit_id.clone());
                                app.begin_edit_source(window, cx);
                            },
                        ))
                        .child(card_action_button(
                            cx,
                            format!("theme-delete-{id}").into(),
                            "icons/trash-2.svg",
                            t(locale, "delete"),
                            true,
                            move |app, _, cx| {
                                app.pending_delete = Some(delete_id.clone());
                                app.codex_menu_open = false;
                                cx.notify();
                            },
                        ))
                        .into_any_element()
                } else {
                    div().into_any_element()
                };
                div()
                    .id(SharedString::from(format!("theme-{id}")))
                    .p_3()
                    .rounded_xl()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if active {
                        rgb(palette.accent)
                    } else {
                        rgb(palette.border)
                    })
                    .bg(if active {
                        rgb(palette.accent_soft)
                    } else {
                        rgb(palette.surface)
                    })
                    .hover(|item| item.bg(rgb(palette.surface_hover)))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(visual)
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .w_full()
                                    .truncate()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(palette.text))
                                    .child(theme.name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .line_clamp(2)
                                    .text_color(rgb(palette.muted))
                                    .child(theme.description),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_xs()
                                                    .text_color(if applied {
                                                        rgb(palette.accent)
                                                    } else {
                                                        rgb(palette.muted)
                                                    })
                                                    .child(status_label),
                                            )
                                            .child(div().flex().items_center().gap_1().children(
                                                theme_colors.into_iter().map(|color| {
                                                    div()
                                                        .size(px(9.))
                                                        .rounded_full()
                                                        .border_1()
                                                        .border_color(rgb(palette.border))
                                                        .bg(rgb(color))
                                                }),
                                            )),
                                    )
                                    .child(actions),
                            ),
                    )
                    .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                        app.selected = Some(id.clone());
                        app.settings_open = false;
                        app.editing_source = false;
                        app.codex_menu_open = false;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let detail_panel: AnyElement = if self.settings_open {
            div()
                .flex_1()
                .p_6()
                .flex()
                .flex_col()
                .gap_5()
                .child(div().text_2xl().child(t(locale, "settings")))
                .child(
                    div()
                        .max_w(px(680.))
                        .p_5()
                        .rounded_xl()
                        .bg(rgb(palette.surface))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(div().text_lg().child(t(locale, "language")))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(palette.muted))
                                .child(t(locale, "language_help")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(button(
                                    cx,
                                    t(locale, "follow_system"),
                                    self.language == Language::System,
                                    |app, _, cx| app.set_language(Language::System, cx),
                                ))
                                .child(button(
                                    cx,
                                    t(locale, "english"),
                                    self.language == Language::English,
                                    |app, _, cx| app.set_language(Language::English, cx),
                                ))
                                .child(button(
                                    cx,
                                    t(locale, "simplified_chinese"),
                                    self.language == Language::SimplifiedChinese,
                                    |app, _, cx| app.set_language(Language::SimplifiedChinese, cx),
                                )),
                        ),
                )
                .child(
                    div()
                        .max_w(px(680.))
                        .p_5()
                        .rounded_xl()
                        .bg(rgb(palette.surface))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(div().text_lg().child(t(locale, "codeface_appearance")))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(palette.muted))
                                .child(t(locale, "appearance_help")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(button(
                                    cx,
                                    t(locale, "light"),
                                    self.appearance == Appearance::Light,
                                    |app, _, cx| app.set_appearance(Appearance::Light, cx),
                                ))
                                .child(button(
                                    cx,
                                    t(locale, "dark"),
                                    self.appearance == Appearance::Dark,
                                    |app, _, cx| app.set_appearance(Appearance::Dark, cx),
                                )),
                        ),
                )
                .into_any_element()
        } else if self.editing_source {
            let image_label = self
                .draft_image
                .as_ref()
                .map(|path| match locale {
                    Locale::SimplifiedChinese => format!("背景图：{}", path.display()),
                    Locale::English => format!("Background: {}", path.display()),
                })
                .unwrap_or_else(|| t(locale, "white_background").into());
            let mut image_row = div().flex().items_center().gap_3();
            if let Some(path) = self.draft_image.as_ref().filter(|path| path.is_file()) {
                image_row = image_row.child(img(path.clone()).w(px(72.)).h(px(48.)).rounded_lg());
            } else {
                image_row = image_row.child(
                    div()
                        .w(px(72.))
                        .h(px(48.))
                        .rounded_lg()
                        .bg(rgb(0xFFFFFF))
                        .border_1()
                        .border_color(rgb(0xD8DCE6)),
                );
            }
            image_row =
                image_row.child(div().text_sm().text_color(rgb(0xAAB2C5)).child(image_label));
            div()
                .flex_1()
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().text_xl().child(if self.editing_id.is_some() {
                            t(locale, "edit_source")
                        } else {
                            t(locale, "new_source")
                        }))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(button(cx, t(locale, "back"), false, |app, _, cx| {
                                    app.editing_source = false;
                                    cx.notify();
                                }))
                                .child(button(
                                    cx,
                                    t(locale, "choose_image"),
                                    false,
                                    |app, _, cx| app.choose_source_image(cx),
                                ))
                                .child(button(cx, t(locale, "copy_prompt"), false, |app, _, cx| {
                                    app.copy_context_prompt(cx)
                                }))
                                .when(self.editing_id.is_some(), |actions| {
                                    actions.child(button(
                                        cx,
                                        t(locale, "delete"),
                                        false,
                                        |app, _, cx| {
                                            app.pending_delete = app.editing_id.clone();
                                            cx.notify();
                                        },
                                    ))
                                })
                                .child(button(cx, t(locale, "save"), false, |app, _, cx| {
                                    app.begin_save_source(false, cx)
                                }))
                                .child(button(cx, t(locale, "save_apply"), true, |app, _, cx| {
                                    app.begin_save_source(true, cx)
                                })),
                        ),
                )
                .child(image_row)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(div().text_lg().child("theme.json"))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .rounded_lg()
                                        .overflow_hidden()
                                        .child(Input::new(&self.theme_json_editor).h_full()),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(div().text_lg().child("codeface.css"))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h_0()
                                        .rounded_lg()
                                        .overflow_hidden()
                                        .child(Input::new(&self.css_editor).h_full()),
                                ),
                        ),
                )
                .child(
                    div()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(0x191C27))
                        .text_sm()
                        .text_color(rgb(0xAAB2C5))
                        .child(self.status.clone()),
                )
                .into_any_element()
        } else {
            let selected_swatches: AnyElement = selected_theme
                .as_ref()
                .map(|theme| {
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .children(
                            [theme.background, theme.panel, theme.accent, theme.text]
                                .into_iter()
                                .map(|color| {
                                    div()
                                        .size(px(14.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgb(palette.border))
                                        .bg(rgb(color))
                                }),
                        )
                        .into_any_element()
                })
                .unwrap_or_else(|| div().into_any_element());
            let preview = if let Some(theme) = selected_theme {
                preview_workspace(theme, locale)
            } else {
                div()
                    .flex_1()
                    .rounded_xl()
                    .bg(rgb(0xFFFFFF))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0x777777))
                    .child(t(locale, "select_theme"))
                    .into_any_element()
            };
            div()
                .flex_1()
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(selected_name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(palette.muted))
                                        .child(t(locale, "preview_help")),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(selected_swatches)
                                .when(selected_is_market, |actions| {
                                    actions
                                        .child(button(
                                            cx,
                                            t(locale, "check_updates"),
                                            false,
                                            |app, _, cx| app.begin_check_theme_update(cx),
                                        ))
                                        .child(button(
                                            cx,
                                            t(locale, "update_theme"),
                                            false,
                                            |app, _, cx| {
                                                if let Ok(id) = app.selected_id() {
                                                    app.begin_install_codexthemes_value(id, cx);
                                                }
                                            },
                                        ))
                                })
                                .when(selected_is_custom, |actions| {
                                    actions
                                        .child(button(
                                            cx,
                                            t(locale, "rollback_theme"),
                                            false,
                                            |app, _, cx| app.begin_rollback_theme(cx),
                                        ))
                                        .child(button(
                                            cx,
                                            t(locale, "export_theme"),
                                            false,
                                            |app, _, cx| app.begin_export_theme(cx),
                                        ))
                                })
                                .child(
                                    div()
                                        .px_3()
                                        .py_2()
                                        .rounded_full()
                                        .bg(rgb(palette.surface_hover))
                                        .text_xs()
                                        .text_color(rgb(palette.muted))
                                        .child(t(locale, "fit_window")),
                                ),
                        ),
                )
                .child(preview)
                .into_any_element()
        };

        div()
            .relative()
            .size_full()
            .bg(rgb(palette.background))
            .text_color(rgb(palette.text))
            .flex()
            .flex_col()
            .when(cfg!(target_os = "macos"), |root| {
                root.child(macos_titlebar())
            })
            .child(
                div()
                    .h(px(56.))
                    .px_5()
                    .border_b_1()
                    .border_color(rgb(palette.border))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .size(px(28.))
                                    .rounded_lg()
                                    .bg(rgb(palette.accent))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("C"),
                            )
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("CodeFace"),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(palette.muted))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().size(px(7.)).rounded_full().bg(if self.busy {
                                        rgb(0xF59E0B)
                                    } else {
                                        rgb(0x34D399)
                                    }))
                                    .child(self.status.clone()),
                            )
                            .child(icon_button(
                                cx,
                                "app-menu",
                                "icons/settings.svg",
                                t(locale, "settings"),
                                |app, _, cx| {
                                    app.codex_menu_open = !app.codex_menu_open;
                                    app.add_theme_menu_open = false;
                                    cx.notify();
                                },
                            )),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(
                        div()
                            .w(px(296.))
                            .p_4()
                            .border_r_1()
                            .border_color(rgb(palette.border))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_end()
                                    .child(div().text_xs().text_color(rgb(palette.muted)).child(
                                        format!(
                                            "{} {}",
                                            self.themes.len(),
                                            t(locale, "themes_count")
                                        ),
                                    ))
                                    .child(
                                        div()
                                            .relative()
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(icon_button(
                                                cx,
                                                "refresh-themes",
                                                "icons/refresh-cw.svg",
                                                t(locale, "refresh"),
                                                |app, _, cx| app.refresh_themes(cx),
                                            ))
                                            .child(icon_button(
                                                cx,
                                                "add-theme-menu",
                                                "icons/plus.svg",
                                                t(locale, "new_theme"),
                                                |app, _, cx| {
                                                    app.add_theme_menu_open =
                                                        !app.add_theme_menu_open;
                                                    app.codex_menu_open = false;
                                                    cx.notify();
                                                },
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("theme-list")
                                    .flex_1()
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .children(cards),
                            ),
                    )
                    .child(detail_panel),
            )
            .when(show_apply_bar, |root| {
                root.child(
                    div()
                        .h(px(68.))
                        .px_5()
                        .border_t_1()
                        .border_color(rgb(palette.border))
                        .bg(rgb(palette.surface))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(palette.muted))
                                        .child(t(locale, "currently_applied")),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(applied_name.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(div().text_sm().text_color(rgb(palette.muted)).child(
                                    format!(
                                        "{}: {}",
                                        t(locale, "selected_theme"),
                                        selected_name.clone()
                                    ),
                                ))
                                .child(
                                    div()
                                        .id("apply-selected-theme")
                                        .px_5()
                                        .py_3()
                                        .rounded_lg()
                                        .cursor_pointer()
                                        .bg(if selection_is_applied || self.busy {
                                            rgb(0x343849)
                                        } else {
                                            rgb(palette.accent)
                                        })
                                        .text_color(if selection_is_applied || self.busy {
                                            rgb(0x9AA1B3)
                                        } else {
                                            rgb(0xFFFFFF)
                                        })
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .hover(|item| item.opacity(0.88))
                                        .child(if self.busy {
                                            t(locale, "applying")
                                        } else if selection_is_applied {
                                            t(locale, "already_applied")
                                        } else {
                                            t(locale, "apply_to_codex")
                                        })
                                        .on_click(cx.listener(|app, _, _, cx| {
                                            if !app.busy {
                                                app.begin_switch(cx);
                                            }
                                        })),
                                ),
                        ),
                )
            })
            .when(add_theme_menu_open, |root| {
                root.child(
                    div()
                        .id("add-theme-dropdown")
                        .absolute()
                        .top(if cfg!(target_os = "macos") {
                            px(152.)
                        } else {
                            px(114.)
                        })
                        .left(px(36.))
                        .w(px(244.))
                        .p_2()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(palette.border))
                        .bg(rgb(palette.surface))
                        .shadow_lg()
                        .occlude()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(button(
                            cx,
                            t(locale, "new_theme"),
                            false,
                            |app, window, cx| app.begin_new_source(window, cx),
                        ))
                        .child(button(cx, t(locale, "import_pack"), false, |app, _, cx| {
                            app.begin_import_pack(cx)
                        }))
                        .child(button(
                            cx,
                            t(locale, "install_from_codexthemes"),
                            false,
                            |app, _, cx| {
                                app.add_theme_menu_open = false;
                                app.codexthemes_open = true;
                                app.codexthemes_error = None;
                                app.market_results.clear();
                                app.pending_delete = None;
                                cx.notify();
                            },
                        )),
                )
            })
            .when(codex_menu_open, |root| {
                root.child(
                    div()
                        .absolute()
                        .top(if cfg!(target_os = "macos") {
                            px(88.)
                        } else {
                            px(50.)
                        })
                        .right(px(20.))
                        .w(px(180.))
                        .p_2()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(palette.border))
                        .bg(rgb(palette.surface))
                        .shadow_lg()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(button(cx, t(locale, "settings"), false, |app, _, cx| {
                            app.settings_open = true;
                            app.editing_source = false;
                            app.codex_menu_open = false;
                            cx.notify();
                        }))
                        .child(button(
                            cx,
                            t(locale, "restart_codex"),
                            false,
                            |app, _, cx| {
                                app.codex_menu_open = false;
                                app.begin_restart_codex(cx)
                            },
                        ))
                        .child(button(cx, t(locale, "close_codex"), false, |app, _, cx| {
                            app.codex_menu_open = false;
                            app.begin_close_codex(cx)
                        })),
                )
            })
            .when_some(pending_delete_name, |root, name| {
                root.child(
                    div()
                        .id("delete-theme-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude()
                        .bg(rgba(0x080A0FCC))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("delete-theme-dialog")
                                .occlude()
                                .w(px(420.))
                                .p_6()
                                .rounded_xl()
                                .border_1()
                                .border_color(rgb(palette.border))
                                .bg(rgb(palette.surface))
                                .flex()
                                .flex_col()
                                .gap_4()
                                .child(div().text_2xl().child(t(locale, "delete_title")))
                                .child(div().text_lg().text_color(rgb(palette.text)).child(name))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(palette.muted))
                                        .child(t(locale, "delete_help")),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(button_with_id(
                                            cx,
                                            "cancel-delete-theme".into(),
                                            t(locale, "cancel"),
                                            false,
                                            |app, _, cx| {
                                                app.pending_delete = None;
                                                cx.notify();
                                            },
                                        ))
                                        .child(button_with_id(
                                            cx,
                                            "confirm-delete-theme".into(),
                                            t(locale, "delete"),
                                            true,
                                            |app, _, cx| app.confirm_delete(cx),
                                        )),
                                ),
                        ),
                )
            })
            .when(self.codexthemes_open, |root| {
                root.child(
                    div()
                        .id("codexthemes-install-overlay")
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .occlude()
                        .bg(rgba(0x080A0FCC))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("codexthemes-install-dialog")
                                .occlude()
                                .w(px(520.))
                                .p_6()
                                .rounded_xl()
                                .border_1()
                                .border_color(rgb(palette.border))
                                .bg(rgb(palette.surface))
                                .flex()
                                .flex_col()
                                .gap_4()
                                .child(
                                    div()
                                        .text_2xl()
                                        .child(t(locale, "install_from_codexthemes")),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(palette.muted))
                                        .child(t(locale, "codexthemes_install_help")),
                                )
                                .child(
                                    div()
                                        .h(px(42.))
                                        .flex()
                                        .gap_2()
                                        .child(
                                            div().flex_1().rounded_lg().overflow_hidden().child(
                                                Input::new(&self.codexthemes_input).h_full(),
                                            ),
                                        )
                                        .child(button_with_id(
                                            cx,
                                            "search-codexthemes".into(),
                                            t(locale, "search"),
                                            false,
                                            |app, _, cx| app.begin_search_codexthemes(cx),
                                        )),
                                )
                                .children(self.codexthemes_error.clone().map(|message| {
                                    div()
                                        .p_3()
                                        .rounded_lg()
                                        .bg(rgba(0x7F1D1DDD))
                                        .text_sm()
                                        .text_color(rgb(0xFECACA))
                                        .child(message)
                                }))
                                .children((!self.market_results.is_empty()).then(|| {
                                    div()
                                        .id("codexthemes-results")
                                        .max_h(px(300.))
                                        .overflow_y_scroll()
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .children(self.market_results.clone().into_iter().map(
                                            |market_theme| {
                                                let id = market_theme.id.clone();
                                                let installable = market_theme.installable;
                                                div()
                                                    .p_3()
                                                    .rounded_lg()
                                                    .border_1()
                                                    .border_color(rgb(palette.border))
                                                    .bg(rgb(palette.surface_hover))
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_3()
                                                    .child(
                                                        div()
                                                            .min_w_0()
                                                            .flex()
                                                            .flex_col()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .font_weight(
                                                                        gpui::FontWeight::SEMIBOLD,
                                                                    )
                                                                    .child(market_theme.name),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(rgb(palette.muted))
                                                                    .line_clamp(2)
                                                                    .child(format!(
                                                                        "{} · {}",
                                                                        market_theme.author,
                                                                        market_theme.description
                                                                    )),
                                                            ),
                                                    )
                                                    .child(button_with_id(
                                                        cx,
                                                        format!("install-market-{id}").into(),
                                                        if installable {
                                                            t(locale, "install")
                                                        } else {
                                                            t(locale, "not_installable")
                                                        },
                                                        installable,
                                                        move |app, _, cx| {
                                                            if installable {
                                                                app.begin_install_codexthemes_value(
                                                                    id.clone(),
                                                                    cx,
                                                                );
                                                            }
                                                        },
                                                    ))
                                            },
                                        ))
                                }))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(palette.muted))
                                        .child("https://codexthemes.ai/zh"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap_2()
                                        .child(button_with_id(
                                            cx,
                                            "cancel-codexthemes-install".into(),
                                            t(locale, "cancel"),
                                            false,
                                            |app, _, cx| {
                                                app.codexthemes_open = false;
                                                app.codexthemes_error = None;
                                                cx.notify();
                                            },
                                        ))
                                        .child(button_with_id(
                                            cx,
                                            "confirm-codexthemes-install".into(),
                                            t(locale, "install"),
                                            true,
                                            |app, _, cx| app.begin_install_codexthemes(cx),
                                        )),
                                ),
                        ),
                )
            })
    }
}

fn main() {
    let arguments: Vec<String> = env::args().collect();
    let cli_locale = i18n::load().effective();
    if arguments
        .get(1)
        .is_some_and(|value| value == "--injector-daemon")
    {
        let result = arguments
            .get(2)
            .ok_or_else(|| anyhow!("Missing CDP port"))
            .and_then(|value| value.parse::<u16>().map_err(Into::into))
            .and_then(|port| cdp::daemon(port, &paths::active_theme_root()?));
        if let Err(error) = result {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(command) = arguments.get(1).map(String::as_str) {
        let result: Option<Result<String>> = match command {
            "--search-codexthemes" => Some((|| {
                let query = arguments.get(2).map(String::as_str).unwrap_or_default();
                Ok(serde_json::to_string_pretty(&theme::search_codexthemes(
                    query,
                )?)?)
            })()),
            "--install-codexthemes" => Some((|| {
                let source = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing CodexThemes theme ID or URL"))?;
                install_codexthemes_checked(source)
                    .map(|id| format!("Installed CodexThemes theme: {id}"))
            })()),
            "--check-theme-update" => Some((|| {
                let id = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing theme ID"))?;
                Ok(serde_json::to_string_pretty(&serde_json::json!({
                    "id": id,
                    "installedVersion": theme::installed_market_version(id)?,
                    "latestVersion": theme::market_version(id)?,
                    "updateAvailable": theme::market_update_available(id)?
                }))?)
            })()),
            "--list-theme-backups" => Some((|| {
                let id = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing theme ID"))?;
                Ok(serde_json::to_string_pretty(&theme::list_backups(id)?)?)
            })()),
            "--rollback-theme" => Some((|| {
                let id = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing theme ID"))?;
                let backup = rollback_theme_checked(id)?;
                Ok(format!(
                    "Restored theme {id} from {}",
                    backup.path.display()
                ))
            })()),
            "--export-theme" => Some((|| {
                let id = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing theme ID"))?;
                theme::export_theme(id).map(|path| format!("Exported theme: {}", path.display()))
            })()),
            "--delete-theme" => Some((|| {
                let id = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing theme ID"))?;
                theme::delete(id).map(|()| format!("Deleted theme: {id}"))
            })()),
            "--import-theme" => Some((|| {
                let source = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing theme directory"))?;
                theme::import_directory(PathBuf::from(source).as_path())
                    .map(|id| format!("Imported theme: {id}"))
            })()),
            "--apply-theme" => Some((|| {
                let id = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing theme ID"))?;
                let (state, health) = apply_theme_checked(id, false)?;
                Ok(serde_json::to_string_pretty(&serde_json::json!({
                    "state": state,
                    "health": health
                }))?)
            })()),
            "--apply-active" => Some((|| {
                let active = paths::active_theme_root()?;
                let value: Value =
                    serde_json::from_str(&fs::read_to_string(active.join("theme.json"))?)?;
                let name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("CodeFace")
                    .to_owned();
                let state = cdp::apply_active(name, false)?;
                Ok(serde_json::to_string_pretty(&state)?)
            })()),
            "--verify" => Some((|| {
                let port = arguments
                    .get(2)
                    .map(String::as_str)
                    .unwrap_or("9341")
                    .parse::<u16>()?;
                cdp::verify(port)?;
                Ok(format!(
                    "{} {} · 127.0.0.1:{port}",
                    t(cli_locale, "verify_ok"),
                    paths::VERSION
                ))
            })()),
            "--health-check" => Some((|| {
                let id = arguments
                    .get(2)
                    .ok_or_else(|| anyhow!("Missing expected theme ID"))?;
                let port = arguments
                    .get(3)
                    .map(String::as_str)
                    .unwrap_or("9341")
                    .parse::<u16>()?;
                let report = cdp::health_check(port, id)?;
                if !report.healthy {
                    bail!("{}", serde_json::to_string_pretty(&report)?);
                }
                Ok(serde_json::to_string_pretty(&report)?)
            })()),
            "--restore" => Some(cdp::remove_live_skin().map(|()| t(cli_locale, "restored").into())),
            "--print-data-root" => Some(paths::state_root().map(|path| path.display().to_string())),
            _ => None,
        };
        if let Some(result) = result {
            match result {
                Ok(output) => println!("{output}"),
                Err(error) => {
                    eprintln!("{error:#}");
                    std::process::exit(1);
                }
            }
            return;
        }
    }
    Application::new()
        .with_assets(CodeFaceAssets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);
            let bounds = Bounds::centered(None, size(px(1280.), px(820.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(titlebar_options()),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| CodeFaceApp::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_color_accepts_hex_and_falls_back() {
        let valid = Value::String("#A1B2C3".into());
        let invalid = Value::String("rgba(1, 2, 3, .5)".into());
        assert_eq!(CodeFaceApp::preview_color(Some(&valid), 0), 0xA1B2C3);
        assert_eq!(
            CodeFaceApp::preview_color(Some(&invalid), 0x123456),
            0x123456
        );
        assert_eq!(CodeFaceApp::preview_color(None, 0xFFFFFF), 0xFFFFFF);
    }

    #[test]
    fn preview_layout_values_are_normalized() {
        let position = Value::String("62% 48%".into());
        let centered = Value::String("center center".into());
        let width = Value::from(272.0);
        let invalid = Value::String("wide".into());

        assert_eq!(CodeFaceApp::preview_position(Some(&position)), (0.62, 0.48));
        assert_eq!(CodeFaceApp::preview_position(Some(&centered)), (0.5, 0.5));
        assert_eq!(CodeFaceApp::preview_position(None), (0.5, 0.5));
        assert_eq!(CodeFaceApp::preview_number(Some(&width), 100.), 272.);
        assert_eq!(CodeFaceApp::preview_number(Some(&invalid), 100.), 100.);
    }
}
