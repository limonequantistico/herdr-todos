//! `herdr-todos toggle`, the plugin action: close the todos panel if this workspace has one,
//! otherwise open it as a narrow right-hand split in the focused pane's folder.

use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

/// The panel's executable name, as herdr reports it for a pane's foreground process.
const EXE: &str = "herdr-todos";
/// Width taken back from the default half-and-half split, as a share of the tab. Leaves the
/// panel at about a third of the width.
const NARROW_BY: &str = "0.17";

pub(crate) fn herdr(args: &[&str]) -> Result<Value> {
    let bin = std::env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".into());
    let out = Command::new(&bin).args(args).output().with_context(|| format!("can't run {bin}"))?;
    if !out.status.success() {
        bail!("herdr {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    serde_json::from_slice(&out.stdout).with_context(|| format!("herdr {}: unreadable output", args.join(" ")))
}

pub(crate) fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, key| v.get(key))?.as_str().filter(|s| !s.is_empty())
}

/// Whether `pane` is running the todos panel (and not, say, this toggle command).
fn is_panel(pane: &str) -> bool {
    let Ok(info) = herdr(&["pane", "process-info", "--pane", pane]) else { return false };
    let procs = info["result"]["process_info"]["foreground_processes"].as_array().cloned().unwrap_or_default();
    procs.iter().any(|p| {
        let argv: Vec<&str> = p["argv"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let exe = p["argv0"].as_str().or(argv.first().copied()).unwrap_or("");
        exe.rsplit('/').next() == Some(EXE) && argv.len() <= 1
    })
}

pub fn run() -> Result<()> {
    let ctx: Value = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let ws = std::env::var("HERDR_WORKSPACE_ID")
        .ok()
        .or_else(|| str_at(&ctx, &["workspace_id"]).map(String::from))
        .context("no workspace: run this from a herdr keybinding or `herdr plugin action invoke`")?;
    let focused = str_at(&ctx, &["focused_pane_id"]).map(String::from).or_else(|| std::env::var("HERDR_PANE_ID").ok());

    let panes = herdr(&["pane", "list", "--workspace", &ws])?;
    let panes = panes["result"]["panes"].as_array().cloned().unwrap_or_default();
    let ids: Vec<&str> = panes.iter().filter_map(|p| p["pane_id"].as_str()).collect();

    let open: Vec<&str> = ids.iter().copied().filter(|id| is_panel(id)).collect();
    if !open.is_empty() {
        for id in open {
            herdr(&["pane", "close", id])?;
        }
        return Ok(());
    }

    // The folder: the focused pane's live directory, else what the context says.
    let target = focused.clone().or_else(|| ids.first().map(|s| s.to_string())).context("no pane to open beside")?;
    let live = panes.iter().find(|p| p["pane_id"].as_str() == Some(&target)).and_then(|p| str_at(p, &["foreground_cwd"]).or(str_at(p, &["cwd"])));
    let dir = live
        .map(String::from)
        .or_else(|| str_at(&ctx, &["focused_pane_cwd"]).map(String::from))
        .or_else(|| str_at(&ctx, &["workspace_cwd"]).map(String::from))
        .context("can't tell which folder you're in")?;

    let plugin = std::env::var("HERDR_PLUGIN_ID").unwrap_or_else(|_| EXE.into());
    let env = format!("{}={dir}", crate::DIR_ENV);
    let opened = herdr(&[
        "plugin", "pane", "open", "--plugin", &plugin, "--entrypoint", "panel", "--placement", "split",
        "--direction", "right", "--target-pane", &target, "--cwd", &dir, "--env", &env, "--focus",
    ])?;
    // Cosmetic: a failed resize leaves a wider panel, never a failed toggle.
    if let Some(pane) = str_at(&opened, &["result", "plugin_pane", "pane", "pane_id"])
        && at_right_edge(pane)
    {
        let _ = herdr(&["pane", "resize", "--pane", pane, "--direction", "right", "--amount", NARROW_BY]);
    }
    Ok(())
}

/// herdr moves a pane's right edge when it has one, and only falls back to its left edge at
/// the right side of the tab. Only there does resizing "right" narrow the panel; anywhere
/// else it would widen it and squeeze the neighbour, so the panel keeps the even split.
fn at_right_edge(pane: &str) -> bool {
    let Ok(layout) = herdr(&["pane", "layout", "--pane", pane]) else { return false };
    let layout = &layout["result"]["layout"];
    let right = |r: &Value| r["x"].as_u64().unwrap_or(0) + r["width"].as_u64().unwrap_or(0);
    let tab_right = right(&layout["area"]);
    layout["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["pane_id"].as_str() == Some(pane))
        .is_some_and(|p| right(&p["rect"]) >= tab_right)
}
