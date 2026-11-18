use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use knave_ui::{HeadingLevel, toolkit::*};
use toml::{Table, Value};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Part {
    Key(String),
    Index(usize),
}

pub type Path = Vec<Part>;

pub fn path(keys: &str) -> Path {
    keys.split('.').map(|key| Part::Key(key.into())).collect()
}

pub fn get<'a>(mut value: &'a Value, path: &[Part]) -> Option<&'a Value> {
    for part in path {
        value = match part {
            Part::Key(key) => value.get(key.as_str())?,
            Part::Index(index) => value.get(*index)?,
        };
    }
    Some(value)
}

pub fn get_mut<'a>(mut value: &'a mut Value, path: &[Part]) -> Option<&'a mut Value> {
    for part in path {
        value = match part {
            Part::Key(key) => value.get_mut(key.as_str())?,
            Part::Index(index) => value.get_mut(*index)?,
        };
    }
    Some(value)
}

pub fn set(root: &mut Value, path: &[Part], value: Option<Value>) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let Some(parent) = get_mut(root, parents) else {
        return;
    };
    match (last, parent, value) {
        (Part::Key(key), Value::Table(table), Some(value)) => {
            table.insert(key.clone(), value);
        }
        (Part::Key(key), Value::Table(table), None) => {
            table.remove(key);
        }
        (Part::Index(index), Value::Array(items), Some(value)) => items[*index] = value,
        (Part::Index(index), Value::Array(items), None) => {
            items.remove(*index);
        }
        _ => {}
    }
}

fn schema_key(path: &[Part]) -> String {
    path.iter()
        .map(|part| match part {
            Part::Key(key) => key.as_str(),
            Part::Index(_) => "*",
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn child(path: &[Part], part: Part) -> Path {
    let mut child = path.to_vec();
    child.push(part);
    child
}

pub fn id(path: &[Part], role: &str) -> u64 {
    let mut hash = DefaultHasher::new();
    (path, role).hash(&mut hash);
    hash.finish() | (1 << 63)
}

pub fn title(text: &str) -> String {
    let mut result = text.replace(['_', '-'], " ");
    if let Some(first) = result.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    result
}

#[derive(Clone)]
pub enum Op {
    Pick(crate::apps::List, Option<Path>),
    Toggle(Path),
    Choose(Path, Vec<Option<Value>>),
    Edit(Path, Value, bool),
    Add(Path, Value),
    Remove(Path),
    Move(Path, usize, usize),
    Expand(Path),
}

#[derive(Clone)]
pub struct Edit {
    pub text: String,
    pub kind: Value,
    pub optional: bool,
}

impl Edit {
    pub fn value(&self) -> crate::config::Result<Option<Value>> {
        let input = self.text.trim();
        if self.optional && input.is_empty() {
            return Ok(None);
        }
        Ok(Some(match self.kind {
            Value::Integer(_) => Value::Integer(input.parse()?),
            Value::Float(_) => {
                let value: f64 = input.parse()?;
                if !value.is_finite() {
                    return Err("Enter a finite number.".into());
                }
                Value::Float(value)
            }
            Value::String(_) => Value::String(self.text.clone()),
            _ => return Err("Unsupported field type.".into()),
        }))
    }
}

pub struct Form<'a> {
    pub catalog: &'a knave_apps::Catalog,
    pub draft: &'a Value,
    pub edits: &'a HashMap<Path, Edit>,
    pub expanded: &'a std::collections::HashSet<Path>,
    pub actions: &'a mut HashMap<ElementId, Op>,
}

impl Form<'_> {
    pub fn fields(&mut self, keys: &[&str]) -> crate::config::Result<Vec<Element>> {
        keys.iter()
            .map(|keys| {
                let path = path(keys);
                let label = label(&path);
                self.field(&path, &label, get(self.draft, &path), true)
            })
            .collect()
    }

    fn register(&mut self, id: u64, op: Op) -> u64 {
        self.actions.insert(ElementId(id), op);
        id
    }

    fn field(
        &mut self,
        path: &[Part],
        name: &str,
        value: Option<&Value>,
        open: bool,
    ) -> crate::config::Result<Element> {
        let key = schema_key(path);
        let control = id(path, "control");
        let optional = optional_kind(&key);
        let workspace_options =
            || {
                let count = get(self.draft, &self::path("compositor.workspace.predefined"))
                    .and_then(Value::as_integer)
                    .unwrap_or(1)
                    .clamp(1, 10);
                std::iter::once(("Use default workspace".into(), None))
                    .chain((1..=count).map(|number| {
                        (format!("Workspace {number}"), Some(Value::Integer(number)))
                    }))
                    .collect()
            };
        let options = if key.ends_with(".workspace") && key.starts_with("compositor.window_rules.")
        {
            Some(workspace_options())
        } else {
            choices(&key, optional.as_ref())
        };
        if let Some(options) = options {
            let selected = options
                .iter()
                .position(|(_, option)| option.as_ref() == value);
            let items = options
                .iter()
                .enumerate()
                .map(|(i, (name, _))| MenuItem::option(i as u64, name))
                .collect();
            self.register(
                control,
                Op::Choose(path.to_vec(), options.into_iter().map(|(_, v)| v).collect()),
            );
            return Ok(row(
                path,
                name,
                Element::new(
                    control,
                    Widget::Dropdown {
                        label: value.and_then(Value::as_str).unwrap_or("Choose").into(),
                        items,
                        selected: selected.map(|i| i as u64),
                    },
                ),
            ));
        }

        match value.or(optional.as_ref()) {
            Some(Value::Boolean(checked)) => {
                self.register(control, Op::Toggle(path.to_vec()));
                Ok(Element::new(
                    control,
                    Widget::Toggle {
                        label: name.into(),
                        checked: *checked,
                    },
                ))
            }
            Some(Value::Table(table)) => {
                let expanded = open || self.expanded.contains(path);
                let mut elements = Vec::new();
                if open {
                    elements.push(Element::heading(control, name, HeadingLevel::H2));
                } else {
                    self.register(control, Op::Expand(path.to_vec()));
                    elements.push(Element::new(
                        control,
                        Widget::Selectable {
                            label: format!("{}  {name}", if expanded { "▾" } else { "▸" }),
                            selected: expanded,
                        },
                    ));
                }
                if expanded {
                    elements.extend(self.table(path, table)?);
                }
                Ok(Element::new(id(path, "group"), Widget::Panel)
                    .class(if open {
                        "settings-group"
                    } else {
                        "settings-details"
                    })
                    .children(elements))
            }
            Some(Value::Array(items)) => self.list(path, name, items),
            Some(kind @ (Value::Integer(_) | Value::Float(_) | Value::String(_))) => {
                let display = self
                    .edits
                    .get(path)
                    .map(|edit| edit.text.clone())
                    .unwrap_or_else(|| match value {
                        Some(Value::String(text)) => text.clone(),
                        Some(value) => value.to_string(),
                        None => String::new(),
                    });
                self.register(
                    control,
                    Op::Edit(path.to_vec(), kind.clone(), optional.is_some()),
                );
                let caption = if optional.is_some() {
                    format!("{name} (optional)")
                } else {
                    name.into()
                };
                let mut input = Element::new(control, Widget::TextInput(TextEdit::new(display)?));
                if matches!(
                    key.as_str(),
                    "compositor.window_rules.application.*.app_id"
                        | "compositor.window_rules.rule.*.match.app_id"
                ) {
                    let browse = self.register(
                        id(path, "browse"),
                        Op::Pick(crate::apps::List::Rules, Some(path.to_vec())),
                    );
                    input = Element::new(id(path, "app-field"), Widget::Panel)
                        .class("settings-row")
                        .layout(Layout {
                            flow: Flow::Row,
                            ..Default::default()
                        })
                        .children(vec![
                            input,
                            crate::footer_button(browse, "Choose app…", ButtonVariant::Secondary),
                        ]);
                }
                Ok(row(path, &caption, input))
            }
            _ => Ok(Element::new(control, Widget::Text(name.into()))),
        }
    }

    fn table(&mut self, path: &[Part], table: &Table) -> crate::config::Result<Vec<Element>> {
        let key = schema_key(path);
        let optional = optional_keys(&key);
        let mut keys = table.keys().map(String::as_str).collect::<Vec<_>>();
        keys.extend(
            optional
                .iter()
                .copied()
                .filter(|name| !table.contains_key(*name)),
        );
        let mut result = Vec::new();
        for name in keys {
            let path = child(path, Part::Key(name.into()));
            let caption = if key == "notifications.app" {
                name.to_string()
            } else {
                label(&path)
            };
            let show_group =
                key == "compositor.window_rules.rule.*" && matches!(name, "match" | "set");
            let mut field = self.field(&path, &caption, table.get(name), show_group)?;
            if key == "notifications.app" && self.expanded.contains(&path) {
                let button = self.register(id(&path, "remove"), Op::Remove(path.clone()));
                field.children.push(Element::action_button(
                    button,
                    "Remove policy",
                    ButtonVariant::Destructive,
                ));
            }
            result.push(field);
        }
        Ok(result)
    }

    fn list(
        &mut self,
        path: &[Part],
        name: &str,
        items: &[Value],
    ) -> crate::config::Result<Element> {
        let key = schema_key(path);
        let add_label = match key.as_str() {
            "compositor.window_rules.application" => "Add by window ID",
            "compositor.window_rules.rule" => "Add window rule",
            "compositor.device" => "Add device override",
            "compositor.bind" => "Add shortcut",
            "compositor.gesture.bind" => "Add gesture",
            "compositor.bind.*.args" => "Add argument",
            _ => "Add",
        };
        let add = self.register(id(path, "add"), Op::Add(path.to_vec(), list_item(&key)?));
        let mut children = vec![Element::heading(
            id(path, "heading"),
            name,
            HeadingLevel::H2,
        )];
        let mut add_buttons = Vec::new();
        if key == "compositor.window_rules.application" {
            let choose =
                self.register(id(path, "choose"), Op::Pick(crate::apps::List::Rules, None));
            add_buttons.push(crate::footer_button(
                choose,
                "Choose app",
                ButtonVariant::Primary,
            ));
        }
        add_buttons.push(crate::footer_button(
            add,
            add_label,
            ButtonVariant::Secondary,
        ));
        children.push(
            Element::new(id(path, "add-buttons"), Widget::Panel)
                .class("window-tabs")
                .layout(Layout {
                    flow: Flow::Row,
                    ..Default::default()
                })
                .children(add_buttons),
        );
        if items.is_empty() {
            children.push(Element::new(
                id(path, "empty"),
                Widget::Text("No entries yet.".into()),
            ));
        }
        let is_rule = key.starts_with("compositor.window_rules.");
        for position in 0..items.len() {
            // Newly added rules appear at the top while Knave retains its configured precedence.
            let index = if is_rule {
                items.len() - position - 1
            } else {
                position
            };
            let value = &items[index];
            let item = child(path, Part::Index(index));
            let raw_label = ["keys", "name", "app_id"].iter().find_map(|key| {
                let field = child(&item, Part::Key((*key).into()));
                self.edits
                    .get(&field)
                    .map(|edit| edit.text.as_str())
                    .or_else(|| value.get(*key).and_then(Value::as_str))
                    .filter(|name| !name.is_empty())
            });
            let display = match key.as_str() {
                "compositor.window_rules.application" => {
                    let name = raw_label
                        .map(|id| crate::apps::matches(self.catalog, id).unwrap_or(id))
                        .unwrap_or("New application rule");
                    let workspace = value.get("workspace").and_then(Value::as_integer);
                    workspace
                        .map(|number| format!("{name} · Workspace {number}"))
                        .unwrap_or_else(|| name.into())
                }
                "compositor.window_rules.rule" => raw_label
                    .or_else(|| {
                        value.get("match").and_then(|v| {
                            ["app_id", "title_contains", "title_regex"]
                                .iter()
                                .find_map(|key| v.get(*key)?.as_str())
                        })
                    })
                    .unwrap_or("Window rule")
                    .into(),
                "compositor.bind.*.args" => format!("Argument {}", index + 1),
                _ => raw_label
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Item {}", index + 1)),
            };
            let ordered = !is_rule;
            let buttons = list_buttons(self.actions, path, index, items.len(), ordered, "Remove");
            let mut field = self.field(&item, &display, Some(value), false)?;
            if value.is_table() {
                let header = field.children.remove(0);
                let controls = std::iter::once(header).chain(buttons).collect();
                field.children.insert(
                    0,
                    Element::new(id(&item, "actions"), Widget::Panel)
                        .class("app-row")
                        .layout(Layout {
                            flow: Flow::Row,
                            align: Align::Center,
                            ..Default::default()
                        })
                        .children(controls),
                );
                children.push(field);
            } else {
                children.push(
                    Element::new(id(&item, "actions"), Widget::Panel)
                        .class("app-row")
                        .layout(Layout {
                            flow: Flow::Row,
                            align: Align::Center,
                            ..Default::default()
                        })
                        .children(std::iter::once(field).chain(buttons).collect()),
                );
            }
        }

        Ok(Element::new(id(path, "list"), Widget::Panel)
            .class("settings-group")
            .children(children))
    }
}

pub fn list_buttons(
    actions: &mut HashMap<ElementId, Op>,
    path: &[Part],
    index: usize,
    count: usize,
    ordered: bool,
    remove_label: &str,
) -> Vec<Element> {
    let item = child(path, Part::Index(index));
    let mut buttons = Vec::new();
    for (role, target, label) in [
        ("up", index.checked_sub(1), "Move up"),
        (
            "down",
            (index + 1 < count).then_some(index + 1),
            "Move down",
        ),
    ] {
        if ordered && count > 1 {
            let button = id(&item, role);
            if let Some(target) = target {
                actions.insert(ElementId(button), Op::Move(path.to_vec(), index, target));
            }
            let mut control = crate::small_button(button, label, ButtonVariant::Secondary);
            control.enabled = target.is_some();
            buttons.push(control);
        }
    }
    let remove = id(&item, "remove");
    actions.insert(ElementId(remove), Op::Remove(item));
    buttons.push(crate::small_button(
        remove,
        remove_label,
        ButtonVariant::Ghost,
    ));
    buttons
}

pub fn row(path: &[Part], label: &str, control: Element) -> Element {
    Element::new(id(path, "row"), Widget::Panel)
        .class("settings-row")
        .layout(Layout {
            flow: Flow::Grid { columns: 2 },
            align: Align::Center,
            ..Default::default()
        })
        .children(vec![
            Element::new(id(path, "label"), Widget::Text(label.into())).class("settings-label"),
            control,
        ])
}

fn label(path: &[Part]) -> String {
    let key = schema_key(path);
    match key.as_str() {
        "appearance.theme.theme" => "Custom theme name".into(),
        "compositor.scale" => "Output scale (1–4)".into(),
        "compositor.modkey" => "Primary modifier (MOD)".into(),
        "compositor.bind" => "Custom bindings".into(),
        "compositor.gesture.bind" => "Swipe bindings".into(),
        "compositor.device" => "Device overrides".into(),
        "compositor.bind.*.keys" => "Keys (e.g. MOD+SHIFT+Q)".into(),
        "compositor.bind.*.dispatch" => "Action".into(),
        "compositor.bind.*.args" => "Action arguments".into(),
        "session.startup.*.command" => "Command".into(),
        "compositor.workspace.predefined" => "Permanent workspaces (1–4095)".into(),
        "compositor.master_percent" => "Master width (10–90%)".into(),
        "compositor.window_rules.application" => "Application rules".into(),
        "compositor.window_rules.application.*.app_id"
        | "compositor.window_rules.rule.*.match.app_id" => "Window app ID".into(),
        "compositor.window_rules.rule.*.match" => "Match windows by".into(),
        "compositor.window_rules.rule.*.set" => "Apply these settings".into(),
        "compositor.appearance.border.width" => "Border thickness (px)".into(),
        "compositor.appearance.border.radius" => "Corner radius (px)".into(),
        "compositor.appearance.gaps.inner" => "Gaps between tiled windows (px per side)".into(),
        "compositor.appearance.gaps.outer" => "Additional gaps at the screen edge (px)".into(),
        "compositor.switch_to_new_window_workspace" => {
            "Follow new windows to their assigned workspace".into()
        }
        "compositor.focus_cycle_order" => "Window switching order".into(),
        "compositor.window_rules.rule" => "Specific window rules".into(),
        "notifications.app" => "Application policies".into(),
        "quick_settings.night_light.start" => "Start (minutes after midnight)".into(),
        "quick_settings.night_light.end" => "End (minutes after midnight)".into(),
        "quick_settings.night_light.temperature" => "Color temperature (K)".into(),
        _ => path
            .last()
            .and_then(|part| match part {
                Part::Key(key) => Some(title(key)),
                Part::Index(_) => None,
            })
            .unwrap_or_else(|| "Item".into()),
    }
}

fn optional_keys(key: &str) -> &'static [&'static str] {
    match key {
        "appearance.theme" => &["theme"],
        "compositor.device.*" => &[
            "enabled",
            "tap_to_click",
            "tap_and_drag",
            "drag_lock",
            "natural_scroll",
            "disable_while_typing",
            "middle_emulation",
            "left_handed",
            "sensitivity",
            "accel_profile",
            "scroll_method",
            "scroll_button",
            "click_method",
        ],
        "compositor.window_rules.application.*" | "compositor.window_rules.rule.*.set" => {
            &["placement", "center", "stay_on_top", "workspace"]
        }
        "compositor.window_rules.rule.*" => &["name"],
        "compositor.window_rules.rule.*.match" => &["app_id", "title_contains", "title_regex"],
        "quick_settings" => &["charge_limit"],
        "quick_settings.night_light" => &["start", "end"],
        _ => &[],
    }
}

fn optional_kind(key: &str) -> Option<Value> {
    let (parent, name) = key.rsplit_once('.')?;
    if !optional_keys(parent).contains(&name) && key != "compositor.environment_file" {
        return None;
    }
    Some(match name {
        "charge_limit" | "start" | "end" | "scroll_button" | "workspace" => Value::Integer(0),
        "sensitivity" => Value::Float(0.0),
        "enabled"
        | "tap_to_click"
        | "tap_and_drag"
        | "drag_lock"
        | "natural_scroll"
        | "disable_while_typing"
        | "middle_emulation"
        | "left_handed"
        | "center"
        | "stay_on_top" => Value::Boolean(false),
        _ => Value::String(String::new()),
    })
}

fn choices(key: &str, optional: Option<&Value>) -> Option<Vec<(String, Option<Value>)>> {
    let values: &[&str] = match key {
        "appearance.theme.mode" => &["system", "dark", "light"],
        "appearance.theme.contrast" => &["standard", "high"],
        "appearance.theme.chroma" => &["full-color", "monochrome"],
        "shell.wallpaper.fit" => &["cover", "contain", "stretch"],
        "compositor.focus_cycle_order" => {
            return Some(vec![
                ("Layout order".into(), Some(Value::String("layout".into()))),
                (
                    "Most recently used".into(),
                    Some(Value::String("mru".into())),
                ),
            ]);
        }
        "compositor.modkey" => &["Super", "Alt"],
        "compositor.device.*.accel_profile" => &["adaptive", "flat"],
        "compositor.device.*.scroll_method" => {
            &["no_scroll", "two_finger", "edge", "on_button_down"]
        }
        "compositor.device.*.click_method" => &["button_areas", "clickfinger"],
        "compositor.bind.*.dispatch" => &[
            "close",
            "exec",
            "focus-down",
            "focus-left",
            "focus-next",
            "focus-previous",
            "focus-right",
            "focus-up",
            "last-workspace",
            "maximize",
            "minimize",
            "move-down",
            "move-left",
            "move-right",
            "move-to-workspace",
            "move-up",
            "move-window",
            "next-workspace",
            "previous-workspace",
            "quit",
            "reset-master",
            "resize-master",
            "resize-window",
            "restore-minimized",
            "screenshot",
            "toggle-floating",
            "toggle-maximize",
            "toggle-overview",
            "unmaximize",
            "workspace",
        ],
        "compositor.gesture.bind.*.direction" => &["horizontal"],
        "compositor.gesture.bind.*.action" => &["workspace", "none"],
        "compositor.window_rules.application.*.placement"
        | "compositor.window_rules.rule.*.set.placement" => &["tiled", "floating"],
        "compositor.window_rules.apply" => &["on_open"],
        "session.backend" => &["auto", "tty", "winit"],
        _ if optional.is_some_and(Value::is_bool) => {
            return Some(vec![
                ("Inherit".into(), None),
                ("Enabled".into(), Some(Value::Boolean(true))),
                ("Disabled".into(), Some(Value::Boolean(false))),
            ]);
        }
        _ => return None,
    };
    let mut items = Vec::new();
    if optional.is_some() {
        items.push(("Inherit".into(), None));
    }
    items.extend(
        values
            .iter()
            .map(|value| (title(value), Some(Value::String((*value).into())))),
    );
    Some(items)
}

fn list_item(key: &str) -> crate::config::Result<Value> {
    let source = match key {
        "compositor.bind" => "keys = ''\ndispatch = 'toggle-floating'\nargs = []",
        "compositor.device" => "name = ''",
        "compositor.gesture.bind" => "fingers = 3\ndirection = 'horizontal'\naction = 'workspace'",
        "compositor.window_rules.application" => "app_id = ''",
        "compositor.window_rules.rule" => "[match]\n[set]",
        "session.startup" => "command = ['']",
        _ => return Ok(Value::String(String::new())),
    };
    Ok(toml::from_str(source)?)
}

pub fn field_name(path: &[Part]) -> String {
    label(path)
}
