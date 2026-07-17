mod cdp;
mod i18n;
mod paths;
mod platform;
mod theme;

use anyhow::{Result, anyhow};
use gpui::{
    AnyElement, App, Application, Bounds, ClipboardItem, Context, ObjectFit, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, img, prelude::*, px, rgb, rgba,
    size,
};
#[cfg(target_os = "macos")]
use gpui::{MouseButton, point};
use gpui_component::{
    Root,
    input::{Input, InputState},
};
use i18n::{Appearance, Language, Locale, t};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, fs, path::PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ThemeSummary {
    id: String,
    name: String,
    image: PathBuf,
    is_system: bool,
    background: u32,
    panel: u32,
    accent: u32,
    text: u32,
    muted: u32,
}

const SYSTEM_THEME_ID: &str = "__codeface-system-theme__";

#[derive(Clone, Copy)]
struct AppPalette {
    background: u32,
    surface: u32,
    surface_hover: u32,
    border: u32,
    text: u32,
    muted: u32,
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
            },
            Self::Dark => AppPalette {
                background: 0x10121A,
                surface: 0x191C27,
                surface_hover: 0x252938,
                border: 0x2A2E3D,
                text: 0xDCE1ED,
                muted: 0x8B93A7,
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
    language: Language,
    appearance: Appearance,
    pending_delete: Option<String>,
    editing_source: bool,
    editing_id: Option<String>,
    draft_image: Option<PathBuf>,
    theme_json_editor: gpui::Entity<InputState>,
    css_editor: gpui::Entity<InputState>,
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
            language,
            appearance,
            pending_delete: None,
            editing_source: false,
            editing_id: None,
            draft_image: None,
            theme_json_editor,
            css_editor,
        };
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
            let image_name = value
                .get("image")
                .and_then(Value::as_str)
                .unwrap_or("background.jpg");
            let colors = value.get("colors");
            self.themes.push(ThemeSummary {
                id,
                name,
                image: dir.join(image_name),
                is_system: false,
                background: Self::preview_color(colors.and_then(|v| v.get("background")), 0xFFFFFF),
                panel: Self::preview_color(colors.and_then(|v| v.get("panel")), 0xFFFFFF),
                accent: Self::preview_color(colors.and_then(|v| v.get("accent")), 0x7C3AED),
                text: Self::preview_color(colors.and_then(|v| v.get("text")), 0x222222),
                muted: Self::preview_color(colors.and_then(|v| v.get("muted")), 0x777777),
            });
        }
        self.themes.sort_by_key(|theme| theme.name.to_lowercase());
        self.themes.insert(
            0,
            ThemeSummary {
                id: SYSTEM_THEME_ID.into(),
                name: t(self.locale(), "system_theme").into(),
                image: PathBuf::new(),
                is_system: true,
                background: 0xFFFFFF,
                panel: 0xF7F7F8,
                accent: 0xD8D8DC,
                text: 0x202123,
                muted: 0x6E6E73,
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

    fn set_editor_text(
        editor: &gpui::Entity<InputState>,
        value: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        editor.update(cx, |editor, cx| editor.set_value(value, window, cx));
    }

    fn begin_new_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_source = true;
        self.settings_open = false;
        self.editing_id = None;
        self.draft_image = None;
        Self::set_editor_text(&self.theme_json_editor, theme::DEFAULT_JSON, window, cx);
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
                    let active = theme::activate(&id)?;
                    let value: Value =
                        serde_json::from_str(&fs::read_to_string(active.join("theme.json"))?)?;
                    cdp::apply_active(
                        value
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or(&id)
                            .to_owned(),
                        false,
                    )?;
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
            move || {
                let active = theme::activate(&id)?;
                let value: Value =
                    serde_json::from_str(&fs::read_to_string(active.join("theme.json"))?)?;
                cdp::apply_active(
                    value
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_owned(),
                    false,
                )
            },
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
            self.status = t(self.locale(), "select_theme").into();
            cx.notify();
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
        match theme::delete(&id) {
            Ok(()) => {
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

fn card_action_button(
    cx: &Context<CodeFaceApp>,
    id: SharedString,
    label: &'static str,
    danger: bool,
    handler: impl Fn(&mut CodeFaceApp, &mut Window, &mut Context<CodeFaceApp>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px_2()
        .py_2()
        .rounded_md()
        .cursor_pointer()
        .bg(rgb(0x252938))
        .text_xs()
        .text_color(if danger { rgb(0xF3A6AE) } else { rgb(0xD8DCE7) })
        .hover(|item| item.opacity(0.84))
        .on_click(cx.listener(move |app, _, window, cx| {
            cx.stop_propagation();
            handler(app, window, cx);
        }))
        .child(label)
        .into_any_element()
}

fn preview_suggestion(icon: &'static str, label: &'static str, panel: u32) -> AnyElement {
    div()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(rgba((panel << 8) | 0xDC))
        .flex()
        .items_center()
        .gap_2()
        .child(icon)
        .child(label)
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
        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
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
        let show_apply_bar = !self.settings_open && !self.editing_source;
        let selection_is_applied = self.selected == self.applied
            || (self.selected.as_deref() == Some(SYSTEM_THEME_ID) && self.applied.is_none());
        let selected = self.selected.clone();
        let selected_theme = self
            .themes
            .iter()
            .find(|theme| selected.as_ref() == Some(&theme.id))
            .cloned();
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
                let visual: AnyElement = if is_system {
                    div()
                        .w(px(72.))
                        .h(px(52.))
                        .rounded_md()
                        .bg(rgb(0xF6F6F7))
                        .border_1()
                        .border_color(rgb(0xD7D7DB))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(0x202123))
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Aa")
                        .into_any_element()
                } else {
                    img(theme.image)
                        .w(px(72.))
                        .h(px(52.))
                        .rounded_md()
                        .object_fit(ObjectFit::Cover)
                        .into_any_element()
                };
                let actions: AnyElement = if !is_system {
                    div()
                        .flex()
                        .gap_1()
                        .child(card_action_button(
                            cx,
                            format!("theme-edit-{id}").into(),
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
                    .p_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if active {
                        rgb(0x8B5CF6)
                    } else {
                        rgb(palette.border)
                    })
                    .bg(if active {
                        rgb(0xEDE9FE)
                    } else {
                        rgb(palette.surface)
                    })
                    .when(self.appearance == Appearance::Dark && active, |item| {
                        item.bg(rgb(0x262139))
                    })
                    .hover(|item| item.bg(rgb(palette.surface_hover)))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(visual)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(rgb(palette.text))
                                    .child(theme.name),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if applied {
                                        rgb(0xB9A3FF)
                                    } else {
                                        rgb(palette.muted)
                                    })
                                    .child(if applied {
                                        t(locale, "currently_applied")
                                    } else if is_system {
                                        t(locale, "system_theme_badge_short")
                                    } else if active {
                                        t(locale, "selected_for_preview")
                                    } else {
                                        t(locale, "custom_theme")
                                    }),
                            ),
                    )
                    .child(actions)
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
            let preview = if let Some(theme) = selected_theme {
                let image = theme.image.clone();
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded_xl()
                    .border_1()
                    .border_color(rgb(theme.accent))
                    .bg(rgb(theme.background))
                    .when(!theme.is_system, |preview| {
                        preview.child(
                            img(image)
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .object_fit(ObjectFit::Cover)
                                .opacity(0.34),
                        )
                    })
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .flex()
                            .child(
                                div()
                                    .w(px(194.))
                                    .px_3()
                                    .py_4()
                                    .bg(rgb(theme.panel))
                                    .text_color(rgb(theme.text))
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .px_2()
                                            .pb_2()
                                            .text_xl()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child("Codex"),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_2()
                                            .rounded_lg()
                                            .bg(rgb(theme.background))
                                            .child(format!("＋  {}", t(locale, "new_task"))),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .child(format!("◷  {}", t(locale, "scheduled"))),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .child(format!("◇  {}", t(locale, "plugins"))),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .child(format!("⌁  {}", t(locale, "pull_requests"))),
                                    )
                                    .child(
                                        div()
                                            .mt_3()
                                            .px_2()
                                            .text_sm()
                                            .text_color(rgb(theme.muted))
                                            .child(t(locale, "projects")),
                                    )
                                    .child(div().px_2().py_1().text_sm().child(theme.name.clone())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px_5()
                                    .py_4()
                                    .flex()
                                    .flex_col()
                                    .text_color(rgb(theme.text))
                                    .child(
                                        div()
                                            .h(px(42.))
                                            .flex()
                                            .items_start()
                                            .justify_between()
                                            .text_sm()
                                            .child(div().child("‹  Codex"))
                                            .child(div().text_color(rgb(theme.muted)).child("⋯")),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .text_3xl()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .child(t(locale, "hero")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mb_3()
                                            .flex()
                                            .justify_center()
                                            .gap_2()
                                            .text_xs()
                                            .text_color(rgb(theme.text))
                                            .child(preview_suggestion(
                                                "◇",
                                                "Build an app",
                                                theme.panel,
                                            ))
                                            .child(preview_suggestion(
                                                "⌘",
                                                "Analyze code",
                                                theme.panel,
                                            ))
                                            .child(preview_suggestion(
                                                "✦",
                                                "Fix a bug",
                                                theme.panel,
                                            ))
                                            .child(preview_suggestion("＋", "More", theme.panel)),
                                    )
                                    .child(
                                        div()
                                            .h(px(106.))
                                            .rounded_xl()
                                            .bg(rgba((theme.panel << 8) | 0xF2))
                                            .border_1()
                                            .border_color(rgb(theme.accent))
                                            .px_4()
                                            .py_3()
                                            .flex()
                                            .flex_col()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
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
                                                            .child("＋")
                                                            .child("⌘"),
                                                    )
                                                    .child(
                                                        div()
                                                            .size(px(30.))
                                                            .rounded_lg()
                                                            .bg(rgb(theme.accent))
                                                            .text_color(rgb(0xFFFFFF))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .child("↑"),
                                                    ),
                                            ),
                                    ),
                            ),
                    )
                    .into_any_element()
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
                                .px_3()
                                .py_2()
                                .rounded_full()
                                .bg(rgb(palette.surface_hover))
                                .text_xs()
                                .text_color(rgb(palette.muted))
                                .child(t(locale, "fit_window")),
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
                                    .bg(rgb(0x7C3AED))
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
                                    .child(self.status.clone()),
                            )
                            .child(div().child(button(
                                cx,
                                t(locale, "codex_controls"),
                                false,
                                |app, _, cx| {
                                    app.codex_menu_open = !app.codex_menu_open;
                                    cx.notify();
                                },
                            )))
                            .child(button(cx, t(locale, "settings"), false, |app, _, cx| {
                                app.settings_open = !app.settings_open;
                                app.editing_source = false;
                                app.codex_menu_open = false;
                                cx.notify();
                            })),
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
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_base()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .child(t(locale, "theme_library")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(rgb(palette.muted))
                                                    .child(format!(
                                                        "{} {}",
                                                        self.themes.len(),
                                                        t(locale, "themes_count")
                                                    )),
                                            ),
                                    )
                                    .child(button(
                                        cx,
                                        t(locale, "new_theme"),
                                        false,
                                        |app, window, cx| app.begin_new_source(window, cx),
                                    )),
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
                            )
                            .child(button(cx, t(locale, "import_pack"), false, |app, _, cx| {
                                app.begin_import_pack(cx)
                            })),
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
                                            rgb(0x7C3AED)
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
            .when(codex_menu_open, |root| {
                root.child(
                    div()
                        .absolute()
                        .top(if cfg!(target_os = "macos") {
                            px(88.)
                        } else {
                            px(50.)
                        })
                        .right(px(112.))
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
    Application::new().run(|cx: &mut App| {
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
}
