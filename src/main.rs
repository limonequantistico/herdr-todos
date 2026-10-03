mod app;
mod clipboard;
mod doc;
mod store;
mod theme;
mod toggle;
mod ui;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use ratatui::crossterm::execute;

use app::App;

/// Set by the toggle action to the folder the user was working in.
const DIR_ENV: &str = "HERDR_TODOS_DIR";

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

    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let result = (|| -> Result<()> {
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
        }
        Ok(())
    })();
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    result
}
