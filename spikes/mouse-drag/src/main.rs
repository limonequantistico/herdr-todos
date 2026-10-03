//! Throwaway spike: in a herdr split pane, can one panel both
//! - drag an item by its grip (⠿) to reorder it, and
//! - drag across item text to select and copy it?
//!
//! Also: click the checkbox to tick, keyboard j/k + J/K + space, t switches look, q to quit.
//! The bottom of the panel logs every raw mouse event, so we can see what herdr forwards.

use std::io::{self, Write};
use std::process::{Command, Stdio};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::{DefaultTerminal, Frame};

const LIST_TOP: u16 = 3; // first item row
const GRIP_END: u16 = 1; // columns 0..=1 are the grip
const CHECK_END: u16 = 5; // columns 2..=5 are "[ ] "
const TEXT_START: u16 = 6;

/// Markdown task syntax, so the panel reads like the `TODOS.md` it edits.
const UNCHECKED: &str = "[ ]";
const CHECKED: &str = "[x]";

struct Item {
    text: String,
    done: bool,
}

enum Gesture {
    None,
    /// `over` is the slot the item would land in; `row` is the pointer row, where the ghost floats.
    Move { from: usize, over: usize, row: u16 },
    Select { anchor: (usize, usize), extent: (usize, usize) },
}

struct App {
    items: Vec<Item>,
    cursor: usize,
    gesture: Gesture,
    log: Vec<String>,
    status: String,
    phosphor: bool,
}

/// The two looks under comparison. `bg: None` keeps the terminal's own background.
struct Theme {
    fg: Color,
    dim: Color,
    bg: Option<Color>,
    /// Text color on a solid `fg` bar (cursor row, ghost).
    on_bar: Color,
    /// Fill of the boxed panels (footer boxes, text selection).
    panel: Color,
}

const PLAIN: Theme =
    Theme { fg: Color::Reset, dim: Color::DarkGray, bg: None, on_bar: Color::Black, panel: Color::Blue };
/// Green phosphor after the Fallout 4 Pip-Boy: the text greens are sampled from a screenshot;
/// the screen is near-black (photos of it read grey only because of glare).
const PHOSPHOR: Theme = Theme {
    fg: Color::Rgb(105, 255, 125),
    dim: Color::Rgb(62, 112, 60),
    bg: Some(Color::Rgb(8, 14, 9)),
    on_bar: Color::Rgb(8, 14, 9),
    panel: Color::Rgb(24, 56, 27),
};

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let result = run(&mut terminal);
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App {
        items: [
            "Buy oat milk",
            "Fix the login redirect bug",
            "Reply to Marco about the offsite",
            "Write release notes for v0.2",
            "Book dentist appointment",
        ]
        .into_iter()
        .map(|t| Item { text: t.into(), done: false })
        .collect(),
        cursor: 0,
        gesture: Gesture::None,
        log: Vec::new(),
        status: "drag ⠿ to move · drag text to select · t: switch look".into(),
        phosphor: true,
    };
    loop {
        terminal.draw(|f| draw(f, &app))?;
        match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('j') | KeyCode::Down => app.cursor = (app.cursor + 1).min(app.items.len() - 1),
                KeyCode::Char('k') | KeyCode::Up => app.cursor = app.cursor.saturating_sub(1),
                KeyCode::Char('J') => app.move_item(app.cursor, app.cursor + 1),
                KeyCode::Char('K') if app.cursor > 0 => app.move_item(app.cursor, app.cursor - 1),
                KeyCode::Char(' ') => app.items[app.cursor].done ^= true,
                KeyCode::Char('t') => app.phosphor ^= true,
                _ => {}
            },
            Event::Mouse(m) => app.on_mouse(m),
            _ => {}
        }
    }
}

impl App {
    fn item_at(&self, row: u16) -> Option<usize> {
        let i = row.checked_sub(LIST_TOP)? as usize;
        (i < self.items.len()).then_some(i)
    }

    fn move_item(&mut self, from: usize, to: usize) {
        let to = to.min(self.items.len() - 1);
        let item = self.items.remove(from);
        self.items.insert(to, item);
        self.cursor = to;
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        self.log.push(format!("{:?} col={} row={} mods={:?}", m.kind, m.column, m.row, m.modifiers));
        if self.log.len() > 8 {
            self.log.remove(0);
        }
        let hit = self.item_at(m.row);
        let chr = m.column.saturating_sub(TEXT_START) as usize;
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(i) = hit else { return };
                self.cursor = i;
                self.gesture = if m.column <= GRIP_END {
                    Gesture::Move { from: i, over: i, row: m.row }
                } else if m.column <= CHECK_END {
                    self.items[i].done ^= true;
                    Gesture::None
                } else {
                    Gesture::Select { anchor: (i, chr), extent: (i, chr) }
                };
            }
            MouseEventKind::Drag(MouseButton::Left) => match &mut self.gesture {
                Gesture::Move { over, row, .. } => {
                    *over = (m.row.saturating_sub(LIST_TOP) as usize).min(self.items.len() - 1);
                    *row = m.row;
                }
                Gesture::Select { extent, .. } => {
                    let row = (m.row.saturating_sub(LIST_TOP) as usize).min(self.items.len() - 1);
                    *extent = (row, chr);
                }
                Gesture::None => {}
            },
            MouseEventKind::Up(MouseButton::Left) => {
                match std::mem::replace(&mut self.gesture, Gesture::None) {
                    Gesture::Move { from, over, .. } if from != over => {
                        self.move_item(from, over);
                        self.status = format!("moved item {} → {}", from + 1, over + 1);
                    }
                    Gesture::Select { anchor, extent } if anchor != extent => {
                        let text = self.selected_text(anchor, extent);
                        self.status = match copy(&text) {
                            Ok(()) => format!("copied: {text:?}"),
                            Err(e) => format!("copy failed: {e}"),
                        };
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn ordered(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
        if a <= b { (a, b) } else { (b, a) }
    }

    /// Character range of `row` covered by the current selection, if any.
    fn selection_on(&self, row: usize) -> Option<(usize, usize)> {
        let Gesture::Select { anchor, extent } = self.gesture else { return None };
        let (s, e) = Self::ordered(anchor, extent);
        (s.0..=e.0).contains(&row).then(|| self.selection_on_range(row, s, e))
    }

    fn selected_text(&self, a: (usize, usize), b: (usize, usize)) -> String {
        let (s, e) = Self::ordered(a, b);
        (s.0..=e.0)
            .map(|row| {
                let (from, to) = self.selection_on_range(row, s, e);
                self.items[row].text.chars().skip(from).take(to - from).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn selection_on_range(&self, row: usize, s: (usize, usize), e: (usize, usize)) -> (usize, usize) {
        let len = self.items[row].text.chars().count();
        let from = if row == s.0 { s.1 } else { 0 };
        let to = if row == e.0 { e.1 + 1 } else { len };
        (from.min(len), to.min(len).max(from.min(len)))
    }
}

fn glyph(done: bool) -> &'static str {
    if done { CHECKED } else { UNCHECKED }
}

fn copy(text: &str) -> io::Result<()> {
    let tool: &[&str] = if cfg!(target_os = "macos") { &["pbcopy"] } else { &["wl-copy"] };
    let mut child = Command::new(tool[0]).stdin(Stdio::piped()).spawn()?;
    child.stdin.take().expect("piped stdin").write_all(text.as_bytes())?;
    child.wait()?;
    Ok(())
}

fn draw(f: &mut Frame, app: &App) {
    let t = if app.phosphor { &PHOSPHOR } else { &PLAIN };
    let base = Style::new().fg(t.fg).bg(t.bg.unwrap_or(Color::Reset));
    let dim = base.fg(t.dim);
    let bar = Style::new().bg(t.fg).fg(t.on_bar).add_modifier(Modifier::BOLD);
    f.render_widget(Block::new().style(base), f.area());

    let [header, _, list, _, footer, status, log] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(app.items.len() as u16 + 1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .areas(f.area());

    // Header. Phosphor: the Pip-Boy tab bar, the active tab bracketed above a rule.
    let title = "SPIKE LIST";
    let header_lines = if app.phosphor {
        let gap = " ".repeat(title.len() + 2);
        let rule = "─".repeat((header.width as usize).saturating_sub(gap.len() + 4));
        vec![
            Line::styled(format!("  ┌ {title} ┐"), base.add_modifier(Modifier::BOLD)),
            Line::styled(format!("──┘{gap}└{rule}"), base),
        ]
    } else {
        vec![Line::styled("● Spike list", base.add_modifier(Modifier::BOLD))]
    };
    f.render_widget(Paragraph::new(header_lines), header);

    let item_line = |i: usize, item: &Item| {
        // Phosphor marks the cursor row with a solid bar, like a Pip-Boy menu selection.
        let on_cursor = app.phosphor && i == app.cursor && !matches!(app.gesture, Gesture::Move { .. });
        let row = if on_cursor { bar } else { base };
        let grip_style = if on_cursor {
            row
        } else if i == app.cursor {
            row.fg(Color::Cyan)
        } else {
            dim
        };
        let mut text_style = if item.done { row.add_modifier(Modifier::CROSSED_OUT) } else { row };
        if item.done && !on_cursor {
            text_style = text_style.fg(t.dim);
        }
        let mut spans = vec![Span::styled("⠿ ", grip_style), Span::styled(format!("{} ", glyph(item.done)), row)];
        match app.selection_on(i) {
            Some((from, to)) => {
                let chars: Vec<char> = item.text.chars().collect();
                let part = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
                // Bright text on the panel fill, even on the cursor row, whose dark bar text
                // would vanish against the fill.
                let picked = text_style.bg(t.panel).fg(t.fg);
                spans.push(Span::styled(part(0, from), text_style));
                spans.push(Span::styled(part(from, to), picked));
                spans.push(Span::styled(part(to, chars.len()), text_style));
            }
            None => spans.push(Span::styled(item.text.clone(), text_style)),
        }
        if on_cursor {
            // Pad so the bar runs the full width of the panel.
            let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            spans.push(Span::styled(" ".repeat((list.width as usize).saturating_sub(used)), row));
        }
        Line::from(spans)
    };

    let lines: Vec<Line> = match app.gesture {
        // While dragging: the list closes up where the item was and opens a dashed slot where it
        // would land, so the drop is previewed before release.
        Gesture::Move { from, over, .. } => {
            let mut lines: Vec<Line> = app
                .items
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != from)
                .map(|(i, item)| item_line(i, item))
                .collect();
            let slot = "┄".repeat(list.width.saturating_sub(2) as usize);
            let slot_style = if app.phosphor { dim } else { base.fg(Color::Cyan) };
            lines.insert(over, Line::styled(format!("  {slot}"), slot_style));
            lines
        }
        _ => app.items.iter().enumerate().map(|(i, item)| item_line(i, item)).collect(),
    };
    f.render_widget(Paragraph::new(lines), list);

    // The ghost: a raised copy of the dragged item that follows the pointer.
    if let Gesture::Move { from, row, .. } = app.gesture {
        let area = f.area();
        let ghost = Rect { x: area.x, y: row.min(area.bottom().saturating_sub(1)), width: area.width, height: 1 };
        let item = &app.items[from];
        let text = format!("⠿ {} {}", glyph(item.done), item.text);
        let style = if app.phosphor { bar } else { Style::new().bg(Color::Cyan).fg(Color::Black).add_modifier(Modifier::BOLD) };
        f.render_widget(Paragraph::new(text).style(style), ghost);
    }

    // Footer. Phosphor: the Pip-Boy status strip (HP | LEVEL [bar] | AP) as three dim-filled
    // boxes with bright text: done count, a progress meter, and the percentage.
    if app.phosphor {
        let done = app.items.iter().filter(|i| i.done).count();
        let total = app.items.len().max(1);
        let boxed = base.bg(t.panel).add_modifier(Modifier::BOLD);
        let left = format!(" DONE {done}/{total} ");
        let right = format!(" {:>3}% ", done * 100 / total);
        let meter_width = (footer.width as usize).saturating_sub(left.len() + right.len() + 6);
        let filled = meter_width * done / total;
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(left, boxed),
                Span::styled(" ", base),
                Span::styled(" ▕", boxed),
                Span::styled("█".repeat(filled), boxed),
                Span::styled(" ".repeat(meter_width - filled), boxed),
                Span::styled("▏ ", boxed),
                Span::styled(" ", base),
                Span::styled(right, boxed),
            ])),
            footer,
        );
    }

    let status_style = if app.phosphor { dim } else { base.fg(Color::Yellow) };
    f.render_widget(Paragraph::new(app.status.as_str()).style(status_style), status);
    f.render_widget(
        Paragraph::new(app.log.iter().map(|l| Line::raw(l.as_str())).collect::<Vec<_>>()).style(dim),
        Rect { height: log.height.min(8), ..log },
    );
}
