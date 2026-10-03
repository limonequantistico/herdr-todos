//! What the panel remembers outside `TODOS.md`, in herdr's state folder for the plugin
//! (`HERDR_PLUGIN_STATE_DIR`):
//!
//! - `state.json`: the theme, and which lists and todos are collapsed in each `TODOS.md`.
//!   Kept across sessions.
//! - `restart-<pid>.json`: undo history, cursor and scroll, handed from a panel to the new
//!   build it restarts on. `exec` keeps the process id, so each panel finds its own file;
//!   it's deleted once read.
//!
//! Losing either file is harmless, so failures to read or write them are ignored.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::doc::ItemRef;

const STATE_FILE: &str = "state.json";

pub struct State {
    dir: PathBuf,
    /// The `TODOS.md` this panel shows: the key for its collapsed set.
    file: String,
}

/// Settings kept across sessions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prefs {
    pub phosphor: bool,
    pub collapsed: HashSet<String>,
}

/// What a panel hands to the build it restarts on.
#[derive(Debug, Default)]
pub struct Handoff {
    /// The file as the old panel last saw it. If it changed during the restart, the undo
    /// history is dropped, so undo never reverts someone else's edit.
    pub text: String,
    /// Undo and redo stacks, as file texts.
    pub history: Vec<String>,
    pub future: Vec<String>,
    pub cursor: Option<ItemRef>,
    pub scroll: usize,
}

impl State {
    /// `None` outside herdr, where there's no state folder: then nothing is remembered.
    pub fn from_env(file: &Path) -> Option<State> {
        let dir = PathBuf::from(std::env::var_os("HERDR_PLUGIN_STATE_DIR")?);
        Some(State::at(dir, file))
    }

    pub fn at(dir: PathBuf, file: &Path) -> State {
        State { dir, file: file.display().to_string() }
    }

    fn read_json(&self, name: &str) -> Option<Value> {
        serde_json::from_slice(&fs::read(self.dir.join(name)).ok()?).ok()
    }

    /// Atomic, like TODOS.md: a temp file, then a rename.
    fn write_json(&self, name: &str, value: &Value) {
        let _ = fs::create_dir_all(&self.dir);
        let tmp = self.dir.join(format!(".{name}.{}.tmp", std::process::id()));
        if fs::write(&tmp, value.to_string()).is_ok() {
            let _ = fs::rename(&tmp, self.dir.join(name));
        }
    }

    pub fn load_prefs(&self) -> Prefs {
        let Some(state) = self.read_json(STATE_FILE) else { return Prefs::default() };
        let collapsed = state["collapsed"][&self.file].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        Prefs { phosphor: state["phosphor"].as_bool().unwrap_or(false), collapsed }
    }

    /// Read again before writing, so panels open on other folders keep their entries.
    /// Entries for files that no longer exist are dropped on the way. The theme is shared by
    /// all panels, so it's written only when this one changed it (`theme_changed`).
    pub fn save_prefs(&self, prefs: &Prefs, theme_changed: bool) {
        // Anything but an object (a hand-edited or half-written file) starts over.
        let mut state = match self.read_json(STATE_FILE) {
            Some(v @ Value::Object(_)) => v,
            _ => json!({}),
        };
        let mut collapsed = match state["collapsed"].take() {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        collapsed.retain(|file, _| Path::new(file).exists());
        let mut keys: Vec<&String> = prefs.collapsed.iter().collect();
        keys.sort();
        if keys.is_empty() {
            collapsed.remove(&self.file);
        } else {
            collapsed.insert(self.file.clone(), json!(keys));
        }
        if theme_changed {
            state["phosphor"] = json!(prefs.phosphor);
        }
        state["collapsed"] = Value::Object(collapsed);
        self.write_json(STATE_FILE, &state);
    }

    fn handoff_file() -> String {
        format!("restart-{}.json", std::process::id())
    }

    pub fn save_handoff(&self, h: &Handoff) {
        let cursor = h.cursor.as_ref().map(|c| json!({ "list": c.list, "path": c.path }));
        let value = json!({ "text": h.text, "history": h.history, "future": h.future, "cursor": cursor, "scroll": h.scroll });
        self.write_json(&State::handoff_file(), &value);
    }

    /// The handoff left for this process, if any. Deleted once read.
    pub fn take_handoff(&self) -> Option<Handoff> {
        let name = State::handoff_file();
        let value = self.read_json(&name);
        let _ = fs::remove_file(self.dir.join(&name));
        let value = value?;
        let texts = |v: &Value| v.as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        let cursor = value["cursor"].as_object().and_then(|c| {
            let list = c.get("list")?.as_u64()? as usize;
            let path = c.get("path")?.as_array()?.iter().map(|n| n.as_u64().map(|n| n as usize)).collect::<Option<Vec<_>>>()?;
            Some(ItemRef { list, path })
        });
        Some(Handoff {
            text: value["text"].as_str().unwrap_or_default().to_string(),
            history: texts(&value["history"]),
            future: texts(&value["future"]),
            cursor,
            scroll: value["scroll"].as_u64().unwrap_or(0) as usize,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("herdr-todos-state-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn prefs_are_kept_per_file_and_the_theme_for_all() {
        let dir = temp("prefs");
        let (a, b) = (dir.join("a.md"), dir.join("b.md"));
        fs::write(&a, "").unwrap();
        fs::write(&b, "").unwrap();
        let (sa, sb) = (State::at(dir.clone(), &a), State::at(dir.clone(), &b));
        sa.save_prefs(&Prefs { phosphor: true, collapsed: HashSet::from(["x".to_string()]) }, true);
        sb.save_prefs(&Prefs { phosphor: false, collapsed: HashSet::from(["y".to_string()]) }, false);
        assert_eq!(sa.load_prefs().collapsed, HashSet::from(["x".to_string()]));
        assert_eq!(sb.load_prefs(), Prefs { phosphor: true, collapsed: HashSet::from(["y".to_string()]) });
        fs::remove_file(&a).unwrap();
        sb.save_prefs(&Prefs::default(), false);
        assert!(sa.load_prefs().collapsed.is_empty(), "a's file is gone: its entry was dropped");
    }

    #[test]
    fn a_state_file_that_is_not_an_object_is_replaced_not_a_crash() {
        let dir = temp("shape");
        fs::write(dir.join(STATE_FILE), "[]").unwrap();
        let s = State::at(dir, Path::new("TODOS.md"));
        s.save_prefs(&Prefs { phosphor: true, collapsed: HashSet::new() }, true);
        assert!(s.load_prefs().phosphor);
    }

    #[test]
    fn a_handoff_is_read_once() {
        let dir = temp("handoff");
        let s = State::at(dir, Path::new("TODOS.md"));
        let cursor = Some(ItemRef { list: 1, path: vec![2, 0] });
        s.save_handoff(&Handoff { text: String::new(), history: vec!["## A\n".into()], future: vec![], cursor: cursor.clone(), scroll: 4 });
        let h = s.take_handoff().unwrap();
        assert_eq!((h.history, h.cursor, h.scroll), (vec!["## A\n".to_string()], cursor, 4));
        assert!(s.take_handoff().is_none());
    }
}
