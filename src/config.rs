use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use knave_config::Config;
use toml::{Table, Value};
use toml_edit::{DocumentMut, Item};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn load(path: &Path) -> Result<Value> {
    let source = read(path)?;
    let typed: Config = toml::from_str(&source)?;
    typed.validate()?;
    draft_from_source(typed, &source)
}

fn draft_from_source(typed: Config, source: &str) -> Result<Value> {
    let source: Value = toml::from_str(source)?;
    let mut result = Value::try_from(typed)?;
    if source
        .get("appearance")
        .and_then(|v| v.get("theme"))
        .and_then(|v| v.get("theme"))
        .is_none()
    {
        result["appearance"]["theme"]
            .as_table_mut()
            .expect("serialized theme table")
            .remove("theme");
    }
    retain_source_fields(&mut result, &source);
    Ok(result)
}

// Keep future settings in list entries even when an entry is edited or moved.
fn retain_source_fields(typed: &mut Value, source: &Value) {
    match (typed, source) {
        (Value::Table(typed), Value::Table(source)) => {
            for (key, value) in source {
                if let Some(typed_value) = typed.get_mut(key) {
                    retain_source_fields(typed_value, value);
                } else {
                    typed.insert(key.clone(), value.clone());
                }
            }
        }
        (Value::Array(typed), Value::Array(source)) => {
            for (typed, source) in typed.iter_mut().zip(source) {
                retain_source_fields(typed, source);
            }
        }
        _ => {}
    }
}

fn read(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        result => result,
    }
}

pub fn save(path: &Path, baseline: &Value, draft: &Value) -> Result<Value> {
    let source = read(path)?;
    let mut document: DocumentMut = source.parse()?;
    let typed: Config = toml::from_str(&source)?;
    let latest = draft_from_source(typed, &source)?;
    let mut keys = Vec::new();
    patch(document.as_table_mut(), &mut keys, baseline, draft, &latest)?;
    let updated = document.to_string();
    let typed: Config = toml::from_str(&updated)?;
    typed.validate()?;
    if baseline.get("compositor").and_then(|v| v.get("bind"))
        != draft.get("compositor").and_then(|v| v.get("bind"))
    {
        for binding in &typed.compositor.bind {
            if binding.keys.trim().is_empty()
                || binding.dispatch.trim().is_empty()
                || (binding.dispatch == "exec"
                    && binding.args.first().is_none_or(|arg| arg.trim().is_empty()))
            {
                return Err(
                    "Every shortcut needs keys and an action, and exec needs a program.".into(),
                );
            }
        }
    }
    if updated != source {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        if let Ok(metadata) = fs::metadata(path) {
            temp.as_file().set_permissions(metadata.permissions())?;
        }
        temp.write_all(updated.as_bytes())?;
        temp.as_file().sync_all()?;
        if read(path)? != source {
            return Err("The configuration changed while saving. Try again.".into());
        }
        temp.persist(path)?;
    }
    draft_from_source(typed, &updated)
}

fn patch(
    document: &mut toml_edit::Table,
    path: &mut Vec<String>,
    baseline: &Value,
    draft: &Value,
    latest: &Value,
) -> Result<()> {
    let empty = Table::new();
    let before = baseline.as_table().unwrap_or(&empty);
    let after = draft.as_table().unwrap_or(&empty);
    for key in before
        .keys()
        .chain(after.keys().filter(|key| !before.contains_key(*key)))
    {
        let old = before.get(key);
        let new = after.get(key);
        if old == new {
            continue;
        }
        let live = latest.get(key);
        path.push(key.clone());
        if let Some(new @ Value::Table(_)) = new {
            if let Some(inline) = document.get(key).and_then(Item::as_inline_table).cloned() {
                document[key] = Item::Table(inline.into_table());
            }
            if !document.contains_key(key) {
                let mut table = toml_edit::Table::new();
                table.set_implicit(true);
                document.insert(key, Item::Table(table));
            }
            let table = document[key]
                .as_table_mut()
                .ok_or("Expected a settings table")?;
            let empty = Value::Table(Table::new());
            patch(
                table,
                path,
                old.unwrap_or(&empty),
                new,
                live.unwrap_or(&empty),
            )?;
        } else {
            if live != old && live != new {
                return Err(format!(
                    "{} was changed elsewhere. Revert to reload the file.",
                    path.join(".")
                )
                .into());
            }
            if let Some(value) = new {
                let serialized =
                    toml_edit::ser::to_document(&Table::from_iter([(key.clone(), value.clone())]))?;
                let mut replacement = serialized[key].clone();
                if let (Some(old), Some(new), Some(existing)) = (
                    old.and_then(Value::as_array),
                    value.as_array(),
                    document.get(key).and_then(Item::as_array_of_tables),
                ) {
                    replacement = Item::ArrayOfTables(patch_array(existing, old, new, path)?);
                }
                if let (Some(old), Some(new)) = (
                    document.get(key).and_then(Item::as_value),
                    replacement.as_value_mut(),
                ) {
                    *new.decor_mut() = old.decor().clone();
                }
                document.insert(key, replacement);
            } else {
                document.remove(key);
            }
        }
        path.pop();
    }
    Ok(())
}

fn patch_array(
    existing: &toml_edit::ArrayOfTables,
    old: &[Value],
    new: &[Value],
    path: &mut Vec<String>,
) -> Result<toml_edit::ArrayOfTables> {
    let mut used = vec![false; old.len()];
    let mut matches = Vec::with_capacity(new.len());
    for entry in new {
        let matching = old
            .iter()
            .enumerate()
            .position(|(index, previous)| !used[index] && previous == entry);
        if let Some(index) = matching {
            used[index] = true;
        }
        matches.push(matching);
    }
    let mut result = toml_edit::ArrayOfTables::new();
    let position = existing.get(0).and_then(toml_edit::Table::position);
    for (index, (entry, matching)) in new.iter().zip(matches).enumerate() {
        let matching =
            matching.or_else(|| (old.len() == new.len() && !used[index]).then_some(index));
        let mut table = if let Some((previous, mut table)) =
            matching.and_then(|index| Some((&old[index], existing.get(index)?.clone())))
        {
            patch(&mut table, path, previous, entry, previous)?;
            table
        } else {
            toml_edit::ser::to_document(entry)?.into_table()
        };
        if let Some(position) = position {
            position_tables(&mut table, position);
        }
        result.push(table);
    }
    Ok(result)
}

fn position_tables(table: &mut toml_edit::Table, position: isize) {
    table.set_position(position);
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(table) => position_tables(table, position),
            Item::ArrayOfTables(tables) => {
                for table in tables.iter_mut() {
                    position_tables(table, position);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::{path, set};

    #[test]
    fn blank_custom_theme_uses_knaves_builtin_theme() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("config.toml");
        fs::write(
            &file,
            "[appearance.theme]\ntheme = 'custom'\nmode = 'light'\n",
        )?;
        let baseline = load(&file)?;
        let mut draft = baseline.clone();
        set(&mut draft, &path("appearance.theme.theme"), None);
        let saved = save(&file, &baseline, &draft)?;
        assert!(crate::forms::get(&saved, &path("appearance.theme.theme")).is_none());
        let typed: Config = saved.try_into()?;
        assert_eq!(typed.appearance.theme_name, "knave");
        assert_eq!(typed.appearance.theme, knave_config::Theme::Light);
        Ok(())
    }

    #[test]
    fn save_merges_external_edits_and_preserves_unknown_rule_fields() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("config.toml");
        let source = "# retained\n[[compositor.device]]\nname = 'Mouse'\nfuture = true\n";
        fs::write(&file, source)?;
        let baseline = load(&file)?;
        let mut draft = baseline.clone();
        let mut rule = path("compositor.device");
        rule.push(crate::forms::Part::Index(0));
        rule.extend(path("left_handed"));
        set(&mut draft, &rule, Some(Value::Boolean(true)));
        fs::write(
            &file,
            format!("{source}\n[portal]\nreduced_motion = true # external\n"),
        )?;
        save(&file, &baseline, &draft)?;
        let saved = fs::read_to_string(&file)?;
        assert!(saved.contains("# retained"));
        assert!(saved.contains("reduced_motion = true # external"));
        assert!(saved.contains("future = true"));
        assert!(saved.contains("left_handed = true"));
        Ok(())
    }

    #[test]
    fn rejects_conflicting_changes_and_removes_explicit_overrides() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("config.toml");
        let original = "[shell.wallpaper]\nimage = '/original.png'\n\n[notifications.app.\"org.example.App\"]\nenabled = false\n";
        fs::write(&file, original)?;
        let baseline = load(&file)?;
        let mut draft = baseline.clone();
        set(&mut draft, &path("shell.wallpaper.image"), None);
        let mut policy = path("notifications.app");
        policy.push(crate::forms::Part::Key("org.example.App".into()));
        set(&mut draft, &policy, None);

        let external = original.replace("/original.png", "/external.png");
        fs::write(&file, &external)?;
        assert!(save(&file, &baseline, &draft).is_err());
        assert_eq!(fs::read_to_string(&file)?, external);

        fs::write(&file, original)?;
        let saved = save(&file, &baseline, &draft)?;
        let typed: Config = saved.try_into()?;
        assert!(typed.shell.wallpaper.image.is_none());
        assert!(typed.notifications.app.is_empty());
        Ok(())
    }
}
