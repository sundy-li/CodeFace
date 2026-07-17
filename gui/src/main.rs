mod cdp;
mod i18n;
mod paths;
mod platform;
mod theme;

use anyhow::{Result, anyhow};
use gpui::{
    AnyElement, App, Application, Bounds, Context, ObjectFit, SharedString, Window, WindowBounds,
    WindowOptions, div, img, prelude::*, px, rgb, size,
};
use gpui_component::{
    Root,
    input::{Input, InputState},
};
use i18n::{Language, Locale, t};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, fs, path::PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ThemeSummary {
    id: String,
    name: String,
    image: PathBuf,
    background: u32,
    panel: u32,
    accent: u32,
    text: u32,
    muted: u32,
}

struct CodeFaceApp {
    themes: Vec<ThemeSummary>,
    selected: Option<String>,
    status: SharedString,
    busy: bool,
    settings_open: bool,
    language: Language,
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
        let mut app = Self {
            themes: Vec::new(),
            selected: None,
            status: t(language.effective(), "ready").into(),
            busy: false,
            settings_open: false,
            language,
            editing_source: false,
            editing_id: None,
            draft_image: None,
            theme_json_editor,
            css_editor,
        };
        app.reload();
        app
    }

    fn locale(&self) -> Locale {
        self.language.effective()
    }

    fn set_language(&mut self, language: Language, cx: &mut Context<Self>) {
        self.language = language;
        self.status = match i18n::save(language) {
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
        let Ok(entries) = fs::read_dir(&root) else {
            self.status = t(self.locale(), "themes_empty").into();
            return;
        };
        for entry in entries.flatten() {
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
                background: Self::preview_color(colors.and_then(|v| v.get("background")), 0xFFFFFF),
                panel: Self::preview_color(colors.and_then(|v| v.get("panel")), 0xFFFFFF),
                accent: Self::preview_color(colors.and_then(|v| v.get("accent")), 0x7C3AED),
                text: Self::preview_color(colors.and_then(|v| v.get("text")), 0x222222),
                muted: Self::preview_color(colors.and_then(|v| v.get("muted")), 0x777777),
            });
        }
        self.themes.sort_by_key(|theme| theme.name.to_lowercase());
        if self
            .selected
            .as_ref()
            .is_none_or(|id| !self.themes.iter().any(|t| &t.id == id))
        {
            self.selected = self.themes.first().map(|theme| theme.id.clone());
        }
        self.status = match self.locale() {
            Locale::SimplifiedChinese => format!("已载入 {} 个主题", self.themes.len()).into(),
            Locale::English => format!("Loaded {} themes", self.themes.len()).into(),
        };
    }

    fn choose_image(&self) -> Result<PathBuf> {
        theme::choose_image(
            t(self.locale(), "select_image_dialog"),
            t(self.locale(), "image_filter"),
        )
        .ok_or_else(|| anyhow!("Selection cancelled"))
    }

    fn choose_theme_pack(&self) -> Result<PathBuf> {
        theme::choose_pack(t(self.locale(), "select_pack_dialog"))
            .ok_or_else(|| anyhow!("Selection cancelled"))
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
        match self.choose_image() {
            Ok(image) => {
                self.draft_image = Some(image.clone());
                self.status = format!(
                    "{}: {}",
                    t(self.locale(), "image_selected"),
                    image.display()
                )
                .into();
            }
            Err(error) => self.status = Self::error_message(self.locale(), &error),
        }
        cx.notify();
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
        let directory = match self.choose_theme_pack() {
            Ok(directory) => directory,
            Err(error) => {
                self.status = Self::error_message(self.locale(), &error);
                cx.notify();
                return;
            }
        };
        let locale = self.locale();
        self.run_operation_async(
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
            |app, result| match result {
                Ok(state) => {
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
                    Ok(()) => t(app.locale(), "codex_closed").into(),
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
                    Ok(()) => t(app.locale(), "codex_restarted").into(),
                    Err(error) => Self::error_message(app.locale(), &error),
                }
            },
            cx,
        );
    }
}

fn button(
    cx: &Context<CodeFaceApp>,
    label: impl Into<SharedString>,
    primary: bool,
    handler: impl Fn(&mut CodeFaceApp, &mut Window, &mut Context<CodeFaceApp>) + 'static,
) -> AnyElement {
    let label = label.into();
    div()
        .id(label.clone())
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

impl Render for CodeFaceApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = self.locale();
        let selected = self.selected.clone();
        let selected_theme = self
            .themes
            .iter()
            .find(|theme| selected.as_ref() == Some(&theme.id))
            .cloned();
        let cards: Vec<AnyElement> = self
            .themes
            .clone()
            .into_iter()
            .map(|theme| {
                let id = theme.id.clone();
                let active = selected.as_ref() == Some(&id);
                div()
                    .id(SharedString::from(format!("theme-{id}")))
                    .p_3()
                    .rounded_xl()
                    .cursor_pointer()
                    .border_1()
                    .border_color(if active { rgb(0xA78BFA) } else { rgb(0x333849) })
                    .bg(if active { rgb(0x29223D) } else { rgb(0x1B1E29) })
                    .child(img(theme.image).w_full().h(px(82.)).rounded_lg())
                    .child(div().text_lg().text_color(rgb(0xF8FAFC)).child(theme.name))
                    .child(div().text_sm().text_color(rgb(0x8B93A7)).child(theme.id))
                    .on_click(
                        cx.listener(move |app, event: &gpui::ClickEvent, window, cx| {
                            app.selected = Some(id.clone());
                            app.settings_open = false;
                            if event.click_count() >= 2 {
                                app.begin_edit_source(window, cx);
                            }
                            cx.notify();
                        }),
                    )
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
                        .bg(rgb(0x191C27))
                        .border_1()
                        .border_color(rgb(0x2A2E3D))
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(div().text_lg().child(t(locale, "language")))
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0x8B93A7))
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
                        .mt_auto()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(0x191C27))
                        .text_sm()
                        .text_color(rgb(0xAAB2C5))
                        .child(self.status.clone()),
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
                    .child(
                        img(image)
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .object_fit(ObjectFit::Cover)
                            .opacity(0.24),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .flex()
                            .child(
                                div()
                                    .w(px(180.))
                                    .p_5()
                                    .bg(rgb(theme.panel))
                                    .text_color(rgb(theme.text))
                                    .flex()
                                    .flex_col()
                                    .gap_4()
                                    .child(div().text_2xl().child("Codex"))
                                    .child(div().text_sm().child(t(locale, "new_task")))
                                    .child(div().text_sm().child(t(locale, "scheduled")))
                                    .child(div().text_sm().child(t(locale, "plugins")))
                                    .child(div().text_sm().child(t(locale, "pull_requests")))
                                    .child(
                                        div()
                                            .mt_4()
                                            .text_sm()
                                            .text_color(rgb(theme.muted))
                                            .child(t(locale, "projects")),
                                    )
                                    .child(div().text_sm().child(theme.name.clone())),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p_6()
                                    .flex()
                                    .flex_col()
                                    .gap_4()
                                    .text_color(rgb(theme.text))
                                    .child(
                                        div()
                                            .h(px(230.))
                                            .rounded_xl()
                                            .bg(rgb(theme.panel))
                                            .border_1()
                                            .border_color(rgb(theme.accent))
                                            .p_6()
                                            .flex()
                                            .flex_col()
                                            .justify_center()
                                            .gap_3()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(rgb(theme.accent))
                                                    .child(theme.name.clone()),
                                            )
                                            .child(div().text_3xl().child(t(locale, "hero")))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(rgb(theme.muted))
                                                    .child(t(locale, "preview_caption")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .mt_auto()
                                            .h(px(112.))
                                            .rounded_xl()
                                            .bg(rgb(theme.panel))
                                            .border_1()
                                            .border_color(rgb(theme.accent))
                                            .p_4()
                                            .flex()
                                            .flex_col()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_color(rgb(theme.muted))
                                                    .child(t(locale, "composer")),
                                            )
                                            .child(
                                                div().flex().justify_between().child("＋").child(
                                                    div()
                                                        .px_3()
                                                        .py_1()
                                                        .rounded_lg()
                                                        .bg(rgb(theme.accent))
                                                        .text_color(rgb(0xFFFFFF))
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
                .p_6()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(div().text_xl().child(t(locale, "theme_preview")))
                        .child(button(cx, t(locale, "apply_theme"), true, |app, _, cx| {
                            app.begin_switch(cx)
                        })),
                )
                .child(preview)
                .into_any_element()
        };

        div()
            .size_full()
            .bg(rgb(0x10121A))
            .text_color(rgb(0xDCE1ED))
            .flex()
            .flex_col()
            .child(
                div()
                    .px_6()
                    .py_5()
                    .border_b_1()
                    .border_color(rgb(0x2A2E3D))
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div().child(div().text_2xl().child("CodeFace")).child(
                            div()
                                .text_sm()
                                .text_color(rgb(0x8B93A7))
                                .child(t(locale, "subtitle")),
                        ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top(px(14.))
                    .right(px(24.))
                    .flex()
                    .gap_2()
                    .child(button(cx, t(locale, "settings"), false, |app, _, cx| {
                        app.settings_open = true;
                        app.editing_source = false;
                        cx.notify();
                    }))
                    .child(button(cx, t(locale, "close_codex"), false, |app, _, cx| {
                        app.begin_close_codex(cx)
                    }))
                    .child(button(
                        cx,
                        t(locale, "restart_codex"),
                        false,
                        |app, _, cx| app.begin_restart_codex(cx),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .overflow_hidden()
                    .child(
                        div()
                            .w(px(340.))
                            .p_5()
                            .border_r_1()
                            .border_color(rgb(0x2A2E3D))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .child(div().text_lg().child(t(locale, "theme_library")))
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .child(button(
                                                cx,
                                                t(locale, "new_theme"),
                                                true,
                                                |app, window, cx| app.begin_new_source(window, cx),
                                            ))
                                            .child(button(
                                                cx,
                                                t(locale, "import_pack"),
                                                false,
                                                |app, _, cx| app.begin_import_pack(cx),
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
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x8B93A7))
                                    .child(t(locale, "double_click")),
                            ),
                    )
                    .child(detail_panel),
            )
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
