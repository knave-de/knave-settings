mod apps;
mod config;
mod forms;
mod wallpaper;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    process::Command,
    sync::Arc,
};

use forms::{Edit, Op, Path, get, get_mut, path, set};
use knave_app::{AppResult, Application, ThemeProvider, ThemeSelection};
use knave_config::{Config, ConfigDocument, WallpaperFit};
use knave_ui::{HeadingLevel, ImageFit, ImageStyle, UiImage, toolkit::*};
use toml::Value;

const SAVE: u64 = 1;
const REVERT: u64 = 2;
const STATUS: u64 = 3;
const BROWSE: u64 = 4;
const DEFAULT_WALLPAPER: u64 = 5;
const BROWSER_INPUT: u64 = 6;
const BROWSER_OPEN: u64 = 7;
const BROWSER_UP: u64 = 8;
const BROWSER_CANCEL: u64 = 9;
const POLICY_INPUT: u64 = 10;
const POLICY_ADD: u64 = 11;
const REFRESH_DEVICES: u64 = 12;
const SESSION_OPTIONS: u64 = 17;
const NAV: u64 = 100;
const WINDOW_TAB: u64 = 150;
const RULE_TAB: u64 = 160;
const BROWSER_ENTRY: u64 = 10_000;

const CSS: &str = "
.settings-nav { background-color: var(--surface-raised); }
.settings-nav selectable { text-align: left; }
.settings-group { padding: 16px; gap: 9px; background-color: var(--surface-raised); border-radius: 10px; }
.settings-row { gap: 14px; background-color: transparent; }
.settings-label { padding: 10px 0px; }
.settings-input { text-align: left; }
.settings-details { padding: 4px 0px 4px 10px; gap: 6px; background-color: transparent; }
.settings-details selectable, .app-option { text-align: left; }
.app-row { padding: 4px 8px; background-color: var(--canvas); border-radius: 6px; }
.window-tabs { background-color: transparent; }
.settings-nav-heading { font-weight: 600; padding: 8px 0px 0px; }
.wallpaper-frame { background-color: #000000; border-radius: 10px; }
.settings-footer { background-color: var(--surface-raised); }
";

struct Page {
    title: &'static str,
    fields: &'static [&'static str],
    hint: &'static str,
}

const PAGES: &[Page] = &[
    Page {
        title: "Appearance",
        fields: &["appearance.theme"],
        hint: "Leave the custom theme name blank to use the built-in Knave theme.",
    },
    Page {
        title: "Display",
        fields: &["shell.wallpaper.fit", "compositor.scale"],
        hint: "",
    },
    Page {
        title: "Windows",
        fields: &[],
        hint: "",
    },
    Page {
        title: "Workspaces",
        fields: &["compositor.workspace"],
        hint: "",
    },
    Page {
        title: "Window rules",
        fields: &[],
        hint: "Choose where an app opens and how its windows are placed. Specific rules can match a window title.",
    },
    Page {
        title: "Input & devices",
        fields: &["compositor.input", "compositor.device"],
        hint: "Device names support * and ?. Later matching overrides take precedence.",
    },
    Page {
        title: "Shortcuts",
        fields: &["compositor.modkey", "compositor.bind"],
        hint: "Custom bindings replace defaults with the same keys. MOD follows the primary modifier.",
    },
    Page {
        title: "Gestures",
        fields: &["compositor.gesture.bind"],
        hint: "By default, a three-finger horizontal swipe switches workspaces. Choose None to pass a gesture to apps.",
    },
    Page {
        title: "Notifications",
        fields: &["notifications"],
        hint: "",
    },
    Page {
        title: "Accessibility",
        fields: &["portal.reduced_motion"],
        hint: "Ask apps that support this preference to use less motion.",
    },
    Page {
        title: "Quick settings",
        fields: &["quick_settings"],
        hint: "Leave both Night Light times empty for manual mode.",
    },
    Page {
        title: "Startup & dock",
        fields: &[],
        hint: "",
    },
];

const RULE_TABS: &[(&str, &[&str])] = &[
    ("By application", &["compositor.window_rules.application"]),
    ("By window title or ID", &["compositor.window_rules.rule"]),
];

const WINDOW_TABS: &[(&str, &[&str])] = &[
    (
        "Behavior",
        &[
            "compositor.focus_cycle_order",
            "compositor.switch_to_new_window_workspace",
            "compositor.master_percent",
        ],
    ),
    (
        "Borders & gaps",
        &[
            "compositor.appearance.border.width",
            "compositor.appearance.border.radius",
            "compositor.appearance.gaps.inner",
            "compositor.appearance.gaps.outer",
        ],
    ),
    (
        "Active windows",
        &[
            "compositor.appearance.focused.opacity",
            "compositor.appearance.focused.border_color",
            "compositor.appearance.focused.blur",
            "compositor.appearance.focused.shadows",
        ],
    ),
    (
        "Inactive windows",
        &[
            "compositor.appearance.unfocused.opacity",
            "compositor.appearance.unfocused.border_color",
            "compositor.appearance.unfocused.blur",
            "compositor.appearance.unfocused.shadows",
        ],
    ),
    (
        "By layout",
        &[
            "compositor.appearance.views.tiled",
            "compositor.appearance.views.floating",
            "compositor.appearance.views.maximized",
        ],
    ),
];

struct Settings {
    scene: Scene,
    file: PathBuf,
    baseline: Value,
    draft: Value,
    edits: HashMap<Path, Edit>,
    expanded: HashSet<Path>,
    actions: HashMap<ElementId, Op>,
    page: usize,
    status: String,
    loaded: bool,
    browser: Option<wallpaper::Browser>,
    import: Option<wallpaper::Import>,
    preview: Option<(String, Result<UiImage, String>)>,
    app_id: String,
    devices: Option<String>,
    catalog: knave_apps::Catalog,
    picker: Option<apps::Picker>,
    window_tab: usize,
    rule_tab: usize,
    session_options: bool,
}

impl Settings {
    fn new(file: PathBuf) -> AppResult<Self> {
        let defaults = Value::try_from(Config::default())?;
        let mut app = Self {
            scene: Scene::new(Element::new(999, Widget::Panel))?,
            file,
            baseline: defaults.clone(),
            draft: defaults,
            edits: HashMap::new(),
            expanded: HashSet::new(),
            actions: HashMap::new(),
            page: 0,
            status: String::new(),
            loaded: false,
            browser: None,
            import: None,
            preview: None,
            app_id: String::new(),
            devices: None,
            catalog: knave_apps::Catalog::load().unwrap_or_default(),
            picker: None,
            window_tab: 0,
            rule_tab: 0,
            session_options: false,
        };
        app.load();
        app.scene.set_stylesheet(CSS)?;
        app.render()?;
        Ok(app)
    }

    fn load(&mut self) {
        match config::load(&self.file) {
            Ok(value) => {
                self.baseline = value.clone();
                self.draft = value;
                self.edits.clear();
                self.import = None;
                self.preview = None;
                self.loaded = true;
                self.status.clear();
            }
            Err(error) => {
                self.loaded = false;
                self.status = format!("Could not load {}. {error}", self.file.display());
            }
        }
    }

    fn flush_edits(&mut self) -> config::Result<()> {
        let edits = self
            .edits
            .iter()
            .map(|(path, edit)| {
                let value = edit
                    .value()
                    .map_err(|error| format!("{}: {error}", forms::field_name(path)))?;
                Ok::<_, Box<dyn std::error::Error>>((path.clone(), value))
            })
            .collect::<config::Result<Vec<_>>>()?;
        for (path, value) in edits {
            set(&mut self.draft, &path, value);
        }
        self.edits.clear();
        Ok(())
    }

    fn save(&mut self) -> config::Result<String> {
        self.flush_edits()?;
        let mut draft = self.draft.clone();
        let imported = self
            .import
            .as_ref()
            .map(wallpaper::Import::persist)
            .transpose()?;
        let saved = (|| {
            if let Some(image) = &imported {
                let image = image
                    .to_str()
                    .ok_or("Wallpaper storage path must be valid UTF-8.")?;
                set(
                    &mut draft,
                    &path("shell.wallpaper.image"),
                    Some(Value::String(image.into())),
                );
            }
            config::save(&self.file, &self.baseline, &draft)
        })();
        let saved = match saved {
            Ok(saved) => saved,
            Err(error) => {
                if let Some(path) = imported {
                    let _ = std::fs::remove_file(path);
                }
                return Err(error);
            }
        };
        self.baseline = saved.clone();
        self.draft = saved;
        self.import = None;
        self.preview = None;
        let output = Command::new("knave").arg("reload").output();
        Ok(match output {
            Ok(result) if result.status.success() => "Settings saved and reloaded.".into(),
            Ok(result) => {
                let error = String::from_utf8_lossy(&result.stderr);
                let detail = if error.trim().is_empty() {
                    result.status.to_string()
                } else {
                    error.trim().into()
                };
                format!("Settings saved. Reload failed: {detail}")
            }
            Err(error) => format!("Settings saved. Could not run knave reload: {error}"),
        })
    }

    fn device_names(&mut self) {
        self.devices = Some(match Command::new("knave").arg("devices").output() {
            Ok(output) if output.status.success() => {
                let names = serde_json::from_slice::<serde_json::Value>(&output.stdout)
                    .ok()
                    .and_then(|value| value.get("payload")?.as_array().cloned())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|entry| entry.get("name")?.as_str().map(str::to_owned))
                    .collect::<Vec<_>>();
                if names.is_empty() {
                    "No input devices reported.".into()
                } else {
                    names.join(" · ")
                }
            }
            _ => "Input devices are unavailable. You can still enter a device name or pattern."
                .into(),
        });
    }

    fn open_path(&mut self, path: PathBuf) -> config::Result<()> {
        if path.is_dir() {
            self.browser = Some(wallpaper::Browser::open(path)?);
        } else {
            self.import = Some(wallpaper::Import::open(&path)?);
            self.browser = None;
        }
        Ok(())
    }

    fn wallpaper(&mut self) -> config::Result<Vec<Element>> {
        let mut typed = Config::default();
        let mut appearance = self.draft["appearance"].clone();
        let theme_name = path("appearance.theme.theme");
        if let Some(edit) = self.edits.get(&theme_name)
            && let Ok(name) = edit.value()
            && name.as_ref().and_then(Value::as_str).is_none_or(|name| {
                name.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        {
            set(&mut appearance, &theme_name[1..], name);
        }
        typed.appearance = appearance.try_into()?;
        typed.shell.wallpaper = self.draft["shell"]["wallpaper"].clone().try_into()?;
        let effective = wallpaper::effective(&typed);
        let (message, image) = if let Some(import) = &self.import {
            (
                format!("Selected for import · {}", import.name),
                Some(import.preview.clone()),
            )
        } else if let Some(effective) = effective {
            let theme_default = effective.source == knave_config::WallpaperSource::ThemeDefault;
            if self
                .preview
                .as_ref()
                .is_some_and(|(path, _)| path != &effective.image)
            {
                self.preview = None;
            }
            let (file, image) = self.preview.get_or_insert_with(|| {
                let image = wallpaper::load_preview(std::path::Path::new(&effective.image))
                    .map_err(|error| error.to_string());
                (effective.image.clone(), image)
            });
            match image {
                Ok(image) => {
                    let label = if theme_default {
                        "Theme default".into()
                    } else {
                        std::path::Path::new(file)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into()
                    };
                    (label, Some(image.clone()))
                }
                Err(error) => (format!("Cannot preview {file}. {error}"), None),
            }
        } else {
            (
                "This theme has no installed default wallpaper.".into(),
                None,
            )
        };
        let fit = match typed.shell.wallpaper.fit {
            WallpaperFit::Cover => ImageFit::Cover,
            WallpaperFit::Contain => ImageFit::Contain,
            WallpaperFit::Stretch => ImageFit::Stretch,
        };
        let image = image.map(|image| {
            Element::new(
                33,
                Widget::Image(
                    image,
                    ImageStyle {
                        fit,
                        ..Default::default()
                    },
                ),
            )
            .layout(Layout {
                height: Length::Fill,
                ..Default::default()
            })
        });
        let mut default = Element::action_button(
            DEFAULT_WALLPAPER,
            "Use theme default",
            ButtonVariant::Secondary,
        );
        default.enabled = self.import.is_some() || typed.shell.wallpaper.image.is_some();
        Ok(vec![
            Element::new(34, Widget::Panel)
                .class("wallpaper-frame")
                .layout(Layout {
                    width: Length::Px(544.0),
                    height: Length::Px(305.0),
                    ..Default::default()
                })
                .children(image.into_iter().collect()),
            text(35, message),
            Element::new(36, Widget::Panel)
                .layout(Layout {
                    flow: Flow::Row,
                    ..Default::default()
                })
                .children(vec![
                    Element::action_button(BROWSE, "Import wallpaper…", ButtonVariant::Secondary),
                    default,
                ]),
        ])
    }

    fn render(&mut self) -> AppResult {
        self.actions.clear();
        let page = &PAGES[self.page];
        let browsing = self.browser.is_some() || self.picker.is_some();
        let (heading, content, scroll) = if let Some(browser) = &self.browser {
            ("Import wallpaper", browser_view(browser)?, 600)
        } else if let Some(picker) = &self.picker {
            let edit = match self.scene.widget(ElementId(apps::SEARCH)) {
                Some(Widget::TextInput(edit)) => edit.clone(),
                _ => TextEdit::new(&picker.query)?,
            };
            (
                picker.list.title(),
                apps::picker(picker, &self.catalog, &self.draft, edit),
                601,
            )
        } else if self.loaded {
            let mut content = Vec::new();
            if !page.hint.is_empty() {
                content.push(text(30, page.hint));
            }
            if self.page == 1 {
                content.extend(self.wallpaper()?);
            }
            if self.page == 5 {
                if let Some(names) = &self.devices {
                    content.push(text(31, names));
                }
                content.push(Element::action_button(
                    REFRESH_DEVICES,
                    "Detect input devices",
                    ButtonVariant::Secondary,
                ));
            }
            let mut fields = page.fields;
            if page.title == "Windows" {
                fields = WINDOW_TABS[self.window_tab].1;
                content.push(tabs(81, WINDOW_TAB, 3, WINDOW_TABS, self.window_tab));
            }
            if page.title == "Window rules" {
                fields = RULE_TABS[self.rule_tab].1;
                content.push(tabs(82, RULE_TAB, 2, RULE_TABS, self.rule_tab));
            }
            if page.title == "Startup & dock" {
                content.extend([apps::List::Startup, apps::List::Dock].map(|list| {
                    apps::managed_list(list, &self.catalog, &self.draft, &mut self.actions)
                }));
                content.push(
                    Element::new(
                        SESSION_OPTIONS,
                        Widget::Selectable {
                            label: if self.session_options {
                                "▾  Advanced session settings"
                            } else {
                                "▸  Advanced session settings"
                            }
                            .into(),
                            selected: self.session_options,
                        },
                    )
                    .class("app-option"),
                );
                if self.session_options {
                    fields = &[
                        "shell.start_bar",
                        "shell.start_overview_service",
                        "session.restart_on_failure",
                        "session.backend",
                        "compositor.environment_file",
                    ];
                }
            }
            let mut form = forms::Form {
                catalog: &self.catalog,
                draft: &self.draft,
                edits: &self.edits,
                expanded: &self.expanded,
                actions: &mut self.actions,
            };
            content.extend(form.fields(fields)?);
            if self.page == 8 {
                content.push(text(37, "Add a policy by application ID"));
                content.push(
                    Element::new(38, Widget::Panel)
                        .layout(Layout {
                            flow: Flow::Row,
                            ..Default::default()
                        })
                        .children(vec![
                            input(POLICY_INPUT, &self.app_id)?,
                            Element::action_button(
                                POLICY_ADD,
                                "Add policy",
                                ButtonVariant::Secondary,
                            ),
                        ]),
                );
            }
            (
                page.title,
                content,
                match page.title {
                    "Windows" => 300 + self.window_tab as u64,
                    "Window rules" => 320 + self.rule_tab as u64,
                    _ => 200 + self.page as u64,
                },
            )
        } else {
            (
                "Configuration unavailable",
                vec![text(
                    39,
                    "Fix the configuration file, then choose Revert to try loading it again.",
                )],
                500,
            )
        };
        let navigation = PAGES
            .iter()
            .enumerate()
            .flat_map(|(index, page)| {
                let group = match index {
                    0 => Some("Desktop"),
                    5 => Some("Input"),
                    8 => Some("System"),
                    _ => None,
                };
                let mut element = Element::new(
                    NAV + index as u64,
                    Widget::Selectable {
                        label: page.title.into(),
                        selected: self.page == index && !browsing,
                    },
                );
                element.enabled = self.loaded;
                group
                    .map(|label| text(910 + index as u64, label).class("settings-nav-heading"))
                    .into_iter()
                    .chain(std::iter::once(element))
            })
            .collect::<Vec<_>>();
        let mut save = footer_button(SAVE, "Save & reload", ButtonVariant::Primary);
        save.enabled = self.loaded;
        let footer_buttons = if self.picker.is_some() {
            vec![footer_button(
                apps::CANCEL,
                "Back",
                ButtonVariant::Secondary,
            )]
        } else if browsing {
            vec![footer_button(
                BROWSER_CANCEL,
                "Cancel import",
                ButtonVariant::Secondary,
            )]
        } else {
            vec![footer_button(REVERT, "Revert", ButtonVariant::Ghost), save]
        };
        let root = Element::new(900, Widget::Panel)
            .layout(Layout {
                height: Length::Fill,
                gap: 0.0,
                ..Default::default()
            })
            .children(vec![
                Element::new(901, Widget::Panel)
                    .layout(Layout {
                        flow: Flow::Row,
                        height: Length::Fill,
                        gap: 0.0,
                        ..Default::default()
                    })
                    .children(vec![
                        Element::new(902, Widget::Scroll)
                            .class("settings-nav")
                            .layout(Layout {
                                width: Length::Px(218.0),
                                height: Length::Fill,
                                padding: 16.0,
                                gap: 6.0,
                                ..Default::default()
                            })
                            .children(
                                std::iter::once(Element::heading(
                                    903,
                                    "Knave Settings",
                                    HeadingLevel::H2,
                                ))
                                .chain(navigation)
                                .collect(),
                            ),
                        Element::new(904, Widget::Panel)
                            .layout(Layout {
                                height: Length::Fill,
                                padding: 22.0,
                                ..Default::default()
                            })
                            .children(vec![
                                Element::heading(905, heading, HeadingLevel::H1),
                                Element::new(scroll, Widget::Scroll)
                                    .layout(Layout {
                                        height: Length::Fill,
                                        gap: 12.0,
                                        ..Default::default()
                                    })
                                    .children(content),
                            ]),
                    ]),
                Element::new(906, Widget::Panel)
                    .class("settings-footer")
                    .layout(Layout {
                        flow: Flow::Row,
                        padding: 10.0,
                        align: Align::Center,
                        ..Default::default()
                    })
                    .children(
                        std::iter::once(text(STATUS, &self.status))
                            .chain(footer_buttons)
                            .collect(),
                    ),
            ]);
        self.scene.replace(root)?;
        Ok(())
    }

    fn remap_list_state(&mut self, list: &[forms::Part], index: impl Fn(usize) -> Option<usize>) {
        let remap = |mut path: Path| {
            if path.starts_with(list)
                && let Some(forms::Part::Index(current)) = path.get_mut(list.len())
            {
                *current = index(*current)?;
            }
            Some(path)
        };
        self.edits = self
            .edits
            .drain()
            .filter_map(|(path, value)| Some((remap(path)?, value)))
            .collect();
        self.expanded = self.expanded.drain().filter_map(remap).collect();
    }

    fn form_action(&mut self, action: Action, op: Op) -> config::Result<bool> {
        match (action, op) {
            (
                Action::TextChanged(_, text) | Action::Cut { text, .. },
                Op::Edit(path, kind, optional),
            ) => {
                self.edits.insert(
                    path,
                    Edit {
                        text,
                        kind,
                        optional,
                    },
                );
                return Ok(false);
            }
            (Action::Toggled(_, checked), Op::Toggle(path)) => {
                set(&mut self.draft, &path, Some(Value::Boolean(checked)))
            }
            (Action::Selected(_, index), Op::Choose(path, options)) => {
                if let Some(value) = options.get(index as usize) {
                    set(&mut self.draft, &path, value.clone());
                    self.preview = None;
                }
            }
            (Action::Activated(_), Op::Expand(path)) => {
                if !self.expanded.remove(&path) {
                    self.expanded.insert(path);
                }
            }
            (Action::Activated(_), Op::Pick(list, replace)) => {
                self.catalog = knave_apps::Catalog::load()?;
                self.picker = Some(apps::Picker {
                    list,
                    query: String::new(),
                    replace,
                });
            }
            (Action::Activated(_), Op::Add(path, value)) => {
                if let Some(items) = get_mut(&mut self.draft, &path).and_then(Value::as_array_mut) {
                    let mut new = path;
                    new.push(forms::Part::Index(items.len()));
                    self.expanded.insert(new);
                    items.push(value);
                }
            }
            (Action::Activated(_), Op::Remove(path)) => {
                if let Some((forms::Part::Index(removed), list)) = path.split_last() {
                    self.remap_list_state(list, |index| match index.cmp(removed) {
                        std::cmp::Ordering::Equal => None,
                        std::cmp::Ordering::Greater => Some(index - 1),
                        std::cmp::Ordering::Less => Some(index),
                    });
                } else {
                    self.edits.retain(|key, _| !key.starts_with(&path));
                    self.expanded.retain(|key| !key.starts_with(&path));
                }
                set(&mut self.draft, &path, None);
            }
            (Action::Activated(_), Op::Move(path, from, to)) => {
                if let Some(items) = get_mut(&mut self.draft, &path).and_then(Value::as_array_mut) {
                    items.swap(from, to);
                    self.remap_list_state(&path, |i| {
                        Some(if i == from {
                            to
                        } else if i == to {
                            from
                        } else {
                            i
                        })
                    });
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn event(&mut self, action: Action, id: ElementId) -> config::Result<bool> {
        if let Some(op) = self.actions.get(&id).cloned() {
            return self.form_action(action, op);
        }
        match action {
            Action::TextChanged(_, text) | Action::Cut { text, .. } if id.0 == apps::SEARCH => {
                if let Some(picker) = &mut self.picker {
                    picker.query = text;
                }
                return Ok(true);
            }
            Action::TextChanged(_, text) | Action::Cut { text, .. } if id.0 == BROWSER_INPUT => {
                if let Some(browser) = &mut self.browser {
                    browser.input = text;
                }
                return Ok(false);
            }
            Action::TextChanged(_, text) | Action::Cut { text, .. } if id.0 == POLICY_INPUT => {
                self.app_id = text;
                return Ok(false);
            }
            Action::Activated(_) | Action::Submitted(_, _) => {}
            _ => return Ok(false),
        }
        self.status.clear();
        match id.0 {
            SAVE => self.status = self.save()?,
            REVERT => self.load(),
            BROWSE => {
                self.browser = Some(wallpaper::Browser::open(
                    wallpaper::Browser::initial_directory(),
                )?)
            }
            DEFAULT_WALLPAPER => {
                self.import = None;
                self.preview = None;
                set(&mut self.draft, &path("shell.wallpaper.image"), None);
            }
            BROWSER_OPEN | BROWSER_INPUT => {
                if let Some(browser) = &self.browser {
                    self.open_path(browser.entered_path())?;
                }
            }
            BROWSER_UP => {
                if let Some(parent) = self
                    .browser
                    .as_ref()
                    .and_then(|browser| browser.directory.parent())
                {
                    self.open_path(parent.to_owned())?;
                }
            }
            BROWSER_CANCEL => self.browser = None,
            POLICY_INPUT | POLICY_ADD => {
                let app_id = self.app_id.trim();
                if app_id.is_empty() {
                    return Err("Enter an application ID.".into());
                }
                let mut policy = path("notifications.app");
                policy.push(forms::Part::Key(app_id.into()));
                if get(&self.draft, &policy).is_some() {
                    return Err("This application already has a policy.".into());
                }
                set(
                    &mut self.draft,
                    &policy,
                    Some(Value::try_from(
                        knave_config::notifications::AppPolicy::default(),
                    )?),
                );
                self.expanded.insert(policy);
                self.app_id.clear();
            }
            REFRESH_DEVICES => self.device_names(),
            apps::CANCEL => self.picker = None,
            SESSION_OPTIONS => self.session_options = !self.session_options,
            number if (WINDOW_TAB..WINDOW_TAB + WINDOW_TABS.len() as u64).contains(&number) => {
                self.window_tab = (number - WINDOW_TAB) as usize;
            }
            number if (RULE_TAB..RULE_TAB + RULE_TABS.len() as u64).contains(&number) => {
                self.rule_tab = (number - RULE_TAB) as usize;
            }
            number if number >= apps::PICK_ENTRY => {
                if let Some(picker) = self.picker.take()
                    && let Some(app) = self.catalog.get((number - apps::PICK_ENTRY) as usize)
                {
                    if let Some(target) = picker.replace {
                        let entry = picker.list.entry(app);
                        set(&mut self.draft, &target, entry.get("app_id").cloned());
                        self.edits.remove(&target);
                        return Ok(true);
                    }
                    let mut path = path(picker.list.key());
                    if let Some(items) =
                        get_mut(&mut self.draft, &path).and_then(Value::as_array_mut)
                    {
                        path.push(forms::Part::Index(items.len()));
                        items.push(picker.list.entry(app));
                        self.expanded.insert(path);
                    }
                }
            }
            number if (NAV..NAV + PAGES.len() as u64).contains(&number) => {
                self.page = (number - NAV) as usize;
                self.browser = None;
                self.picker = None;
            }
            number if number >= BROWSER_ENTRY => {
                if let Some(entry) = self
                    .browser
                    .as_ref()
                    .and_then(|browser| browser.entries.get((number - BROWSER_ENTRY) as usize))
                {
                    self.open_path(entry.clone())?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

impl Application for Settings {
    fn app_id(&self) -> Option<&str> {
        Some("de.knave.Settings")
    }

    fn scene(&mut self) -> &mut Scene {
        &mut self.scene
    }

    fn action(&mut self, action: Action) -> AppResult {
        let id = match &action {
            Action::Activated(id)
            | Action::TextChanged(id, _)
            | Action::Toggled(id, _)
            | Action::Selected(id, _)
            | Action::Submitted(id, _)
            | Action::Cut { id, .. } => *id,
            _ => return Ok(()),
        };
        match self.event(action, id) {
            Ok(true) => self.render(),
            Ok(false) => Ok(()),
            Err(error) => {
                self.status = error.to_string();
                self.render()
            }
        }
    }
}

fn text(id: u64, caption: impl Into<String>) -> Element {
    Element::new(id, Widget::Text(caption.into()))
}

fn tabs(id: u64, base: u64, columns: usize, tabs: &[(&str, &[&str])], selected: usize) -> Element {
    Element::new(id, Widget::Panel)
        .class("window-tabs")
        .layout(Layout {
            flow: Flow::Grid { columns },
            ..Default::default()
        })
        .children(
            tabs.iter()
                .enumerate()
                .map(|(index, (label, _))| {
                    Element::new(
                        base + index as u64,
                        Widget::Selectable {
                            label: (*label).into(),
                            selected: index == selected,
                        },
                    )
                })
                .collect(),
        )
}

fn small_button(id: u64, label: &str, variant: ButtonVariant) -> Element {
    Element::action_button(id, label, variant).layout(Layout {
        width: Length::Px(92.0),
        ..Default::default()
    })
}

fn footer_button(id: u64, label: &str, variant: ButtonVariant) -> Element {
    Element::action_button(id, label, variant).layout(Layout {
        width: Length::Px(148.0),
        ..Default::default()
    })
}

fn input(id: u64, value: &str) -> Result<Element, UiError> {
    Ok(Element::new(id, Widget::TextInput(TextEdit::new(value)?)).class("settings-input"))
}

fn browser_view(browser: &wallpaper::Browser) -> config::Result<Vec<Element>> {
    let mut entries = vec![
        text(40, "Choose a PNG or JPEG, or enter its path."),
        Element::new(41, Widget::Panel)
            .layout(Layout {
                flow: Flow::Row,
                ..Default::default()
            })
            .children(vec![
                input(BROWSER_INPUT, &browser.input)?,
                Element::action_button(BROWSER_OPEN, "Open", ButtonVariant::Primary),
            ]),
        Element::action_button(BROWSER_UP, "Parent folder", ButtonVariant::Ghost),
    ];
    if browser.entries.is_empty() {
        entries.push(text(42, "No folders or supported images here."));
    }
    entries.extend(browser.entries.iter().enumerate().map(|(index, path)| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let caption = if path.is_dir() {
            format!("{name}/")
        } else {
            name.into_owned()
        };
        Element::new(
            BROWSER_ENTRY + index as u64,
            Widget::Selectable {
                label: caption,
                selected: false,
            },
        )
    }));
    Ok(entries)
}

struct DesktopTheme(PathBuf);

impl DesktopTheme {
    fn selected(&self) -> (ThemePolicy, Option<PathBuf>) {
        let document = ConfigDocument::load(&self.0).ok();
        let appearance = document
            .as_ref()
            .map(|d| d.config().appearance.clone())
            .unwrap_or_default();
        let theme = match appearance.theme {
            knave_config::Theme::Light => Theme::Light,
            _ => Theme::Dark,
        };
        let contrast = match appearance.contrast {
            knave_config::ContrastPreference::Standard => Contrast::Standard,
            knave_config::ContrastPreference::High => Contrast::High,
        };
        let chroma = match appearance.chroma {
            knave_config::ChromaPreference::FullColor => Chroma::FullColor,
            knave_config::ChromaPreference::Monochrome => Chroma::Monochrome,
        };
        let stylesheet = theme_file::data_home()
            .ok()
            .and_then(|home| theme_file::theme_css_path(home, &appearance.theme_name, theme).ok());
        (ThemePolicy::new(theme, contrast, chroma), stylesheet)
    }
}

impl ThemeProvider for DesktopTheme {
    fn load(&self) -> Result<ThemeSelection, String> {
        let (policy, stylesheet) = self.selected();
        let stylesheet = stylesheet
            .map(theme_file::load_css)
            .transpose()
            .map_err(|error| error.to_string())?
            .flatten()
            .unwrap_or_default();
        Ok(ThemeSelection { policy, stylesheet })
    }

    fn watch_paths(&self) -> Vec<PathBuf> {
        let (_, stylesheet) = self.selected();
        std::iter::once(self.0.clone()).chain(stylesheet).collect()
    }
}

fn main() -> AppResult {
    let file = ConfigDocument::default_path()?;
    knave_app::run_with_theme_provider(
        "Knave Settings",
        [1040.0, 760.0],
        Settings::new(file.clone())?,
        Arc::new(DesktopTheme(file)),
    )
}
