use std::path::Path;

use knave_apps::{App, Catalog};
use knave_ui::{HeadingLevel, toolkit::*};
use toml::{Table, Value};

use crate::{forms, text};

pub const SEARCH: u64 = 15;
pub const CANCEL: u64 = 16;
pub const PICK_ENTRY: u64 = 1_000_000;

#[derive(Clone, Copy, PartialEq)]
pub enum List {
    Startup,
    Dock,
    Rules,
}

impl List {
    pub fn key(self) -> &'static str {
        match self {
            Self::Startup => "session.startup",
            Self::Dock => "shell.pinned_apps",
            Self::Rules => "compositor.window_rules.application",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Startup => "Choose an app to open at login",
            Self::Dock => "Choose an app to pin",
            Self::Rules => "Choose an app for the rule",
        }
    }

    pub fn entry(self, app: &App) -> Value {
        match self {
            Self::Startup => Value::Table(Table::from_iter([(
                "command".into(),
                Value::Array(app.argv.iter().cloned().map(Value::String).collect()),
            )])),
            Self::Dock => Value::String(app.id.clone()),
            Self::Rules => Value::Table(Table::from_iter([(
                "app_id".into(),
                Value::String(app.id.strip_suffix(".desktop").unwrap_or(&app.id).into()),
            )])),
        }
    }

    fn identify(self, value: &Value, app: &App) -> bool {
        match self {
            Self::Startup => value.get("command") == self.entry(app).get("command"),
            Self::Dock => value
                .as_str()
                .is_some_and(|id| id.eq_ignore_ascii_case(&app.id)),
            Self::Rules => value.get("app_id") == self.entry(app).get("app_id"),
        }
    }
}

pub struct Picker {
    pub list: List,
    pub query: String,
    pub replace: Option<forms::Path>,
}

pub fn matches<'a>(catalog: &'a Catalog, identity: &str) -> Option<&'a str> {
    let stem = |identity: &str| identity.trim_end_matches(".desktop").to_owned();
    let identity = stem(identity);
    (0..catalog.len())
        .filter_map(|i| catalog.get(i))
        .find(|app| {
            stem(&app.id).eq_ignore_ascii_case(&identity)
                || app.name.eq_ignore_ascii_case(&identity)
        })
        .map(|app| app.name.as_str())
}

pub fn picker(picker: &Picker, catalog: &Catalog, draft: &Value, edit: TextEdit) -> Vec<Element> {
    let path = forms::path(picker.list.key());
    let items = forms::get(draft, &path)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut elements = vec![
        text(79, "Search installed applications"),
        Element::new(SEARCH, Widget::TextInput(edit)).class("settings-input"),
    ];
    let matches = if picker.query.trim().is_empty() {
        (0..catalog.len()).collect()
    } else {
        catalog.search(&picker.query, catalog.len())
    };
    if matches.is_empty() {
        elements.push(text(80, "No matching applications."));
    }
    elements.extend(matches.into_iter().filter_map(|index| {
        let app = catalog.get(index)?;
        let selected =
            picker.replace.is_none() && items.iter().any(|value| picker.list.identify(value, app));
        let label = if selected {
            format!("{} · Already added", app.name)
        } else {
            app.name.clone()
        };
        let mut element = Element::new(
            PICK_ENTRY + index as u64,
            Widget::Selectable { label, selected },
        );
        element.enabled = !selected;
        Some(element.class("app-option"))
    }));
    elements
}

pub fn managed_list(
    list: List,
    catalog: &Catalog,
    draft: &Value,
    actions: &mut std::collections::HashMap<ElementId, forms::Op>,
) -> Element {
    let path = forms::path(list.key());
    let items = forms::get(draft, &path)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let (title, hint, add, remove, empty) = match list {
        List::Startup => (
            "Open at login",
            "These apps open the next time you sign in.",
            "Add app",
            "Remove",
            "No apps set to open at login.",
        ),
        List::Dock => (
            "Overview dock",
            "Pinned apps appear in this order in Overview.",
            "Pin app",
            "Unpin",
            "No pinned apps.",
        ),
        List::Rules => unreachable!(),
    };
    let button = forms::id(&path, "pick");
    actions.insert(ElementId(button), forms::Op::Pick(list, None));
    let mut elements = vec![
        Element::heading(forms::id(&path, "heading"), title, HeadingLevel::H2),
        text(forms::id(&path, "hint"), hint),
        crate::footer_button(button, add, ButtonVariant::Secondary),
    ];
    if items.is_empty() {
        elements.push(text(forms::id(&path, "empty"), empty));
    }
    for (index, value) in items.iter().enumerate() {
        let mut item = path.clone();
        item.push(forms::Part::Index(index));
        let name = match list {
            List::Dock => {
                let id = value.as_str().unwrap_or_default();
                matches(catalog, id)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{} (unavailable)", id.trim_end_matches(".desktop")))
            }
            List::Startup => (0..catalog.len())
                .filter_map(|i| catalog.get(i))
                .find(|app| list.identify(value, app))
                .map(|app| app.name.clone())
                .unwrap_or_else(|| {
                    let program = value
                        .get("command")
                        .and_then(|v| v.get(0))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let name = Path::new(program)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(program);
                    format!("{name} (custom startup command)")
                }),
            List::Rules => unreachable!(),
        };
        let mut cells = vec![text(forms::id(&item, "name"), name)];
        cells.extend(forms::list_buttons(
            actions,
            &path,
            index,
            items.len(),
            list == List::Dock,
            remove,
        ));
        elements.push(
            Element::new(forms::id(&item, "app-row"), Widget::Panel)
                .class("app-row")
                .layout(Layout {
                    flow: Flow::Row,
                    align: Align::Center,
                    ..Default::default()
                })
                .children(cells),
        );
    }
    Element::new(forms::id(&path, "list"), Widget::Panel)
        .class("settings-group")
        .children(elements)
}
