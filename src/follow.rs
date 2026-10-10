//! Following the folder: the panel shows the `TODOS.md` of the pane it sits beside, so a `cd`
//! there (or a workspace that moved on to another folder) brings the right todos along.
//!
//! A background thread asks herdr for the panes in this workspace once a minute:
//! one CLI call, off the drawing loop so a slow answer never freezes the panel. It follows the
//! last terminal pane focused in the panel's own tab, or the first one before any was focused,
//! and sends a folder only when it changes.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::toggle::{herdr, str_at};

const EVERY: Duration = Duration::from_secs(60);

/// `None` outside herdr, where there's nothing to follow.
pub fn start(dir: PathBuf) -> Option<Receiver<PathBuf>> {
    let me = std::env::var("HERDR_PANE_ID").ok()?;
    let ws = std::env::var("HERDR_WORKSPACE_ID").ok()?;
    let tab = std::env::var("HERDR_TAB_ID").ok();
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut shown = dir;
        let mut followed: Option<String> = None;
        loop {
            thread::sleep(EVERY);
            let Ok(list) = herdr(&["pane", "list", "--workspace", &ws]) else { continue };
            let panes = list["result"]["panes"].as_array().cloned().unwrap_or_default();
            let Some(next) = pick(&panes, &me, tab.as_deref(), &mut followed) else { continue };
            if next != shown {
                shown = next.clone();
                if tx.send(next).is_err() {
                    return;
                }
            }
        }
    });
    Some(rx)
}

/// The folder to show, from herdr's pane list. `followed` remembers the pane across calls.
fn id(p: &Value) -> &str {
    p["pane_id"].as_str().unwrap_or_default()
}

fn pick(panes: &[Value], me: &str, tab: Option<&str>, followed: &mut Option<String>) -> Option<PathBuf> {
    // The pane may have been moved to another tab since it opened: go by where it is now.
    let tab = panes.iter().find(|p| id(p) == me).and_then(|p| p["tab_id"].as_str()).or(tab)?;
    // Plugin panes (this one, a reviewer…) carry a label; terminals don't. A tab of labelled
    // panes only still has something to follow.
    let beside: Vec<&Value> = panes.iter().filter(|p| p["tab_id"].as_str() == Some(tab) && id(p) != me).collect();
    let terminals: Vec<&Value> = beside.iter().copied().filter(|p| str_at(p, &["label"]).is_none()).collect();
    let candidates = if terminals.is_empty() { beside } else { terminals };
    if let Some(p) = candidates.iter().find(|p| p["focused"].as_bool() == Some(true)) {
        *followed = Some(id(p).to_string());
    }
    let pane = followed.as_deref().and_then(|f| candidates.iter().find(|p| id(p) == f)).or(candidates.first())?;
    str_at(pane, &["foreground_cwd"]).or(str_at(pane, &["cwd"])).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pane(id: &str, tab: &str, cwd: &str, label: Option<&str>, focused: bool) -> Value {
        json!({ "pane_id": id, "tab_id": tab, "foreground_cwd": cwd, "label": label, "focused": focused })
    }

    #[test]
    fn follows_the_terminal_beside_it_not_other_plugin_panes() {
        let panes = [
            pane("p1", "t1", "/elsewhere", None, false),
            pane("p2", "t2", "/rea", None, false),
            pane("p3", "t2", "/cogi", Some("todos"), false),
            pane("p4", "t2", "/cogi", Some("reviewr"), true),
        ];
        assert_eq!(pick(&panes, "p3", None, &mut None), Some(PathBuf::from("/rea")));
    }

    #[test]
    fn sticks_to_the_last_focused_terminal() {
        let mut followed = None;
        let panes = [pane("a", "t", "/one", None, false), pane("b", "t", "/two", None, true), pane("me", "t", "/x", Some("todos"), false)];
        assert_eq!(pick(&panes, "me", None, &mut followed), Some(PathBuf::from("/two")));
        // Focus moves to the panel itself (or another tab): keep showing the last one.
        let panes = [pane("a", "t", "/one", None, false), pane("b", "t", "/two", None, false), pane("me", "t", "/x", Some("todos"), true)];
        assert_eq!(pick(&panes, "me", None, &mut followed), Some(PathBuf::from("/two")));
    }

    #[test]
    fn nothing_beside_it_means_no_change() {
        let panes = [pane("me", "t", "/x", Some("todos"), false)];
        assert_eq!(pick(&panes, "me", Some("t"), &mut None), None);
    }
}
