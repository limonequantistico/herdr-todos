mod app;
mod clipboard;
mod doc;
mod state;
mod store;
mod theme;
mod toggle;
mod ui;

use std::io;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::execute;

use app::App;

/// Set by the toggle action to the folder the user was working in.
const DIR_ENV: &str = "HERDR_TODOS_DIR";
/// Set when the panel restarts itself on a new build, so the new one can say so.
const RESTARTED_ENV: &str = "HERDR_TODOS_RESTARTED";
/// How long the executable must stay unchanged before restarting on it, so a build still
/// being written is never run half-copied.
const SETTLE: Duration = Duration::from_secs(1);

fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("toggle") => toggle::run(),
        Some(other) => anyhow::bail!("unknown command {other:?} (expected no argument, or `toggle`)"),
        None => panel(),
    }
}

fn panel() -> Result<()> {
    let dir = match std::env::var_os(DIR_ENV) {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_dir().context("no working directory")?,
    };
    let mut app = App::open(&dir).with_context(|| format!("can't open TODOS.md in {}", dir.display()))?;
    // Keep the watcher alive for the whole session; without it the panel just won't notice
    // outside edits until the next change it makes itself.
    let watch = store::watch(app.file()).ok();
    app.watching = watch.is_some();

    let restarted = std::env::var_os(RESTARTED_ENV).is_some();
    if let Some(state) = state::State::from_env(app.file()) {
        app.attach_state(state, restarted);
    }
    if restarted {
        app.status = Some("restarted with the new build".into());
    }
    let exe = std::env::current_exe().ok();
    let mut built = exe.as_deref().and_then(modified);
    // A newer executable on disk, and since when it's looked like that.
    let mut newer: Option<(SystemTime, Instant)> = None;
    let mut checked = Instant::now();

    loop {
        let mut terminal = ratatui::init();
        execute!(io::stdout(), EnableMouseCapture)?;
        let result = (|| -> Result<bool> {
            while !app.quit {
                app.set_area(terminal.get_frame().area());
                terminal.draw(|f| ui::draw(f, &app))?;
                if event::poll(Duration::from_millis(200))? {
                    match event::read()? {
                        Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
                        Event::Mouse(m) => app.on_mouse(m),
                        _ => {}
                    }
                }
                if let Some((_, rx)) = &watch
                    && rx.try_iter().count() > 0
                {
                    app.on_file_event();
                }
                app.save_prefs();
                // Look at the executable once a second: one `stat`, nothing more.
                if checked.elapsed() < Duration::from_secs(1) {
                    continue;
                }
                checked = Instant::now();
                // Rebuilt (or updated by herdr): restart on it once it has settled and
                // nothing would be lost, i.e. nothing is being written or dragged.
                let now = exe.as_deref().and_then(modified);
                match (now, newer) {
                    (Some(t), _) if Some(t) == built => newer = None,
                    (Some(t), Some((seen, since))) if t == seen => {
                        if since.elapsed() >= SETTLE && app.can_restart() {
                            return Ok(true);
                        }
                    }
                    (Some(t), _) => newer = Some((t, Instant::now())),
                    (None, _) => {}
                }
            }
            Ok(false)
        })();
        execute!(io::stdout(), DisableMouseCapture)?;
        ratatui::restore();
        if !result? {
            return Ok(());
        }
        // Replace this process with the new build, in the same pane: exec only returns on failure.
        let Some(exe) = &exe else { return Ok(()) };
        app.hand_off();
        let err = Command::new(exe).args(std::env::args_os().skip(1)).env(RESTARTED_ENV, "1").exec();
        app.status = Some(format!("can't restart on the new build: {err}"));
        // Don't retry this build every second; the next one gets a fresh try.
        built = modified(exe);
        newer = None;
    }
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
