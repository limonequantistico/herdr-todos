//! Layout and drawing. `rows` is the single source for both: drawing walks it, and mouse
//! hit-testing reads the same rows, so a click always lands on what's shown.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, Focus, Gesture};
use crate::doc::{Doc, Entry, ItemRef, Place, items_in};
use crate::theme::{PHOSPHOR, PLAIN, Theme};

/// Row 0 is the quick-add input, row 1 a spacer; lists start here.
pub const LIST_TOP: u16 = 2;
/// Per item row, after `SUB_INDENT` columns per level: the grip (2 columns), `[ ] ` (4), text.
const SUB_INDENT: u16 = 2;
/// Narrowest a wrapped line of text gets, however deep the nesting.
const MIN_WRAP: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Title,
    Item,
    /// A new todo being typed, not yet in the file.
    New,
    /// "+ Add a to-do" at the end of a list: click to start writing there.
    AddHere,
    /// "+ New list", after the last list before Done.
    NewList,
    /// Placeholder for an empty Done list.
    Empty,
    Gap,
    /// The drop preview while dragging.
    Slot,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: RowKind,
    pub list: usize,
    /// The item on an `Item` row.
    pub at: Option<ItemRef>,
    pub depth: u16,
    /// This row's piece of the text: long todos wrap over several rows.
    pub text: String,
    /// Where `text` starts in the whole todo, in chars. 0 on the first row of a todo.
    pub offset: usize,
    pub done: bool,
    /// On the first row of a todo with sub-items, at the panel's right edge: `⏷`, or `⏵ 3`
    /// when they're collapsed. Kept clear of the text, so clicking a todo's end edits it.
    pub marker: Option<String>,
    /// The column the marker starts at.
    pub marker_col: u16,
    /// Whether this todo's sub-items are showing below it, so its guide line starts right
    /// under its grip, through its own wrapped rows.
    pub opens: bool,
    /// Where a dragged item lands when dropped on this row: at this row's level, before it.
    pub drop: Place,
}

impl Row {
    pub fn lead(&self) -> u16 {
        SUB_INDENT * self.depth
    }

    pub fn grip_end(&self) -> u16 {
        self.lead() + 1
    }

    pub fn check_end(&self) -> u16 {
        self.lead() + 5
    }

    pub fn text_start(&self) -> u16 {
        self.lead() + 6
    }

    pub fn has_text(&self) -> bool {
        self.kind == RowKind::Item
    }

    /// The first row of a todo, the one with the grip and the box.
    pub fn is_first(&self) -> bool {
        self.offset == 0
    }
}

/// What to lay out besides the document itself.
pub struct View<'a> {
    /// An item being dragged: left out, with everything under it.
    pub skip: Option<&'a ItemRef>,
    /// Where a new todo is being typed.
    pub pending: Option<&'a Place>,
    /// The item being edited; it shows `live` instead of its saved text, so it wraps as you type.
    pub editing: Option<&'a ItemRef>,
    pub live: Option<String>,
    /// Todos whose sub-items are hidden, by `collapse_key`.
    pub collapsed: &'a HashSet<String>,
    /// The panel width, for wrapping.
    pub width: u16,
}

/// Identifies a todo across moves and reloads: its list and the texts down to it.
pub fn collapse_key(doc: &Doc, at: &ItemRef) -> String {
    let mut key = doc.lists.get(at.list).map_or(String::new(), |l| l.name.clone());
    for n in 1..=at.path.len() {
        if let Some(item) = doc.item(&ItemRef { list: at.list, path: at.path[..n].to_vec() }) {
            key.push('\u{1f}');
            key.push_str(&item.text);
        }
    }
    key
}

/// Identifies a collapsed list by its name. Todo keys always contain `\u{1f}`, list keys never
/// do, so the two can share one set.
pub fn list_collapse_key(name: &str) -> String {
    format!("\u{1e}{name}")
}

/// Split `text` into rows of at most `width` chars, breaking after a space where possible.
/// Returns (char offset, piece) pairs; always at least one, so an empty todo still has a row.
pub fn wrap(text: &str, width: usize) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let width = width.max(MIN_WRAP);
    let mut out = Vec::new();
    let mut start = 0;
    while chars.len() - start > width {
        let window = &chars[start..start + width];
        let cut = window.iter().rposition(|&c| c == ' ').filter(|&i| i > 0).map_or(width, |i| i + 1);
        out.push((start, chars[start..start + cut].iter().collect()));
        start += cut;
    }
    out.push((start, chars[start..].iter().collect()));
    out
}

fn count_items(entries: &[Entry]) -> usize {
    entries.iter().map(|e| if let Entry::Item(i) = e { 1 + count_items(&i.children) } else { 0 }).sum()
}

/// The list area's rows, in display order.
pub fn rows(doc: &Doc, view: &View) -> Vec<Row> {
    let order = doc.display_order();
    let mut rows = Vec::new();
    // "+ New list" goes after the last list that isn't Done (or first, if there's none).
    let last_open = order.iter().rposition(|&l| !doc.lists[l].is_done());
    let new_list_row = |list: usize, slot: usize| Row {
        kind: RowKind::NewList,
        list,
        at: None,
        depth: 0,
        text: String::new(),
        offset: 0,
        done: false,
        marker: None,
        marker_col: 0,
        opens: false,
        drop: Place { list, parent: Vec::new(), slot },
    };
    if last_open.is_none() {
        rows.push(new_list_row(order.first().copied().unwrap_or(0), 0));
        if !order.is_empty() {
            rows.push(Row { kind: RowKind::Gap, ..new_list_row(order[0], 0) });
        }
    }
    for (n, &li) in order.iter().enumerate() {
        let list = &doc.lists[li];
        let plain = |kind, text: &str, slot| Row {
            kind,
            list: li,
            at: None,
            depth: 0,
            text: text.to_string(),
            offset: 0,
            done: false,
            marker: None,
            marker_col: 0,
            opens: false,
            drop: Place { list: li, parent: Vec::new(), slot },
        };
        // Lists with todos get a collapse marker at the right edge of their title.
        let count = count_items(&list.entries);
        let collapsed = count > 0 && view.collapsed.contains(&list_collapse_key(&list.name));
        let mut title = plain(RowKind::Title, &list.name, list.start_slot());
        if count > 0 {
            let marker = if collapsed { format!("⏵ {count}") } else { "⏷".to_string() };
            title.marker_col = view.width.saturating_sub(marker.chars().count() as u16 + 1);
            title.marker = Some(marker);
        }
        rows.push(title);
        if !collapsed {
            let before = rows.len();
            push_items(&mut rows, view, &list.entries, li, &[], &list.name);
            if !list.is_done() {
                rows.push(plain(RowKind::AddHere, "", list.end_slot()));
            } else if rows.len() == before {
                rows.push(plain(RowKind::Empty, "", list.end_slot()));
            }
        }
        if last_open == Some(n) {
            rows.push(new_list_row(li, list.end_slot()));
        }
        if n + 1 < order.len() {
            rows.push(plain(RowKind::Gap, "", list.end_slot()));
        }
    }
    rows
}

fn push_items(rows: &mut Vec<Row>, view: &View, entries: &[Entry], list: usize, parent: &[usize], key: &str) {
    let depth = parent.len() as u16;
    let text_start = SUB_INDENT * depth + 6;
    let room = (view.width as usize).saturating_sub(text_start as usize);
    let mut pending = view.pending.filter(|p| p.list == list && p.parent == parent);
    let push_new = |rows: &mut Vec<Row>, place: &Place| {
        for (offset, text) in wrap(view.live.as_deref().unwrap_or(""), room) {
            let drop = place.clone();
            rows.push(Row { kind: RowKind::New, list, at: None, depth, text, offset, done: false, marker: None, marker_col: 0, opens: false, drop });
        }
    };
    for i in items_in(entries) {
        if let Some(place) = pending.take_if(|p| p.slot <= i) {
            push_new(rows, place);
        }
        let Entry::Item(item) = &entries[i] else { continue };
        let mut path = parent.to_vec();
        path.push(i);
        let at = ItemRef { list, path };
        if Some(&at) == view.skip {
            continue;
        }
        let key = format!("{key}\u{1f}{}", item.text);
        let hidden = count_items(&item.children);
        let collapsed = hidden > 0 && view.collapsed.contains(&key);
        let marker = (hidden > 0).then(|| if collapsed { format!("⏵ {hidden}") } else { "⏷".to_string() });
        let saved = item.full_text();
        let text = match (view.editing, &view.live) {
            (Some(e), Some(live)) if *e == at => live.as_str(),
            _ => saved.as_str(),
        };
        // Keep the text clear of the marker at the right edge, with a two-column gap.
        let marker_len = marker.as_ref().map_or(0, |m| m.chars().count());
        let width = room.saturating_sub(if marker_len > 0 { marker_len + 2 } else { 0 });
        let marker_col = view.width.saturating_sub(marker_len as u16 + 1);
        let pieces = wrap(text, width);
        for (n, (offset, piece)) in pieces.into_iter().enumerate() {
            rows.push(Row {
                kind: RowKind::Item,
                list,
                at: Some(at.clone()),
                depth,
                text: piece,
                offset,
                done: item.done,
                marker: if n == 0 { marker.clone() } else { None },
                marker_col,
                opens: hidden > 0 && !collapsed,
                drop: Place { list, parent: parent.to_vec(), slot: i },
            });
        }
        if !collapsed {
            push_items(rows, view, &item.children, list, &at.path, &key);
        }
    }
    if let Some(place) = pending {
        push_new(rows, place);
    }
}

/// Where the drop preview goes in `rows` for a pointer over row `r`: (insert index, whether
/// it replaces the row there — an "empty list" placeholder).
pub fn slot_position(rows: &[Row], r: usize) -> (usize, bool) {
    match rows[r].kind {
        RowKind::Title => (r + 1, matches!(rows.get(r + 1).map(|x| x.kind), Some(RowKind::Empty))),
        RowKind::Empty => (r, true),
        // On a wrapped todo, the slot goes above its first row, never between its rows.
        _ => {
            let mut r = r;
            while r > 0 && !rows[r].is_first() {
                r -= 1;
            }
            (r, false)
        }
    }
}

/// The indent of row `i`: a guide line under each ancestor's grip, like the indent guides in
/// VS Code or IntelliJ. A guide turns into a corner (`╰`) on the last row it covers, so you
/// can see where a block ends. The guide at level `lit` (the current block's) is heavier
/// (`┃`, `┗`) in `lit_style`, so it reads as a different line, not just a colour.
fn guides(rows: &[Row], i: usize, lit: Option<usize>, style: Style, lit_style: Style) -> Vec<Span<'static>> {
    let row = &rows[i];
    (0..row.depth as usize)
        .map(|k| {
            let closes = !rows.get(i + 1).is_some_and(|next| has_guide(next, k));
            match (Some(k) == lit, closes) {
                (true, false) => Span::styled("┃ ", lit_style),
                (true, true) => Span::styled("┗ ", lit_style),
                (false, false) => Span::styled("│ ", style),
                (false, true) => Span::styled("╰ ", style),
            }
        })
        .collect()
}

/// Whether `row` carries the guide at level `k`: every row nested deeper than `k`, and the
/// wrapped rows of a level-`k` todo whose sub-items show (its line starts under its grip).
fn has_guide(row: &Row, k: usize) -> bool {
    matches!(row.kind, RowKind::Item | RowKind::New | RowKind::Slot)
        && (row.depth as usize > k || (row.opens && !row.is_first() && row.depth as usize == k))
}

/// The current block, as (list, path of the todo that owns it). For the todo under the
/// cursor (or being written): its own sub-items if they're showing, so the line right below
/// it lights up; otherwise the block it sits in. `None` at the top level, which has no guide.
fn current_block(app: &App, rows: &[Row]) -> Option<(usize, Vec<usize>)> {
    let at = match &app.focus {
        Focus::New { place, .. } => return (!place.parent.is_empty()).then(|| (place.list, place.parent.clone())),
        Focus::Edit { at, .. } => at,
        _ => app.cursor.as_ref()?,
    };
    let opens_below = rows.iter().any(|r| {
        r.list == at.list
            && match r.kind {
                RowKind::Item => r.at.as_ref().is_some_and(|a| a.path.len() > at.path.len() && a.path.starts_with(&at.path)),
                RowKind::New => r.drop.parent.starts_with(&at.path),
                _ => false,
            }
    });
    if opens_below {
        return Some((at.list, at.path.clone()));
    }
    (at.depth() > 0).then(|| (at.list, at.parent().to_vec()))
}

/// Which guide on `row` belongs to the current block, if any: the rows inside the block's
/// parent light up the guide at the parent's level.
fn lit_guide(row: &Row, block: &Option<(usize, Vec<usize>)>) -> Option<usize> {
    let (list, parent) = block.as_ref()?;
    let path = match row.kind {
        RowKind::Item => &row.at.as_ref()?.path,
        RowKind::New | RowKind::Slot => &row.drop.parent,
        _ => return None,
    };
    // New and slot rows carry their parent's path; item rows their own, one longer.
    let inside = row.list == *list && path.starts_with(parent) && (path.len() > parent.len() || row.kind != RowKind::Item);
    inside.then(|| parent.len() - 1)
}

pub fn theme(app: &App) -> &'static Theme {
    if app.phosphor { &PHOSPHOR } else { &PLAIN }
}

/// How many list rows fit between the spacer and the status line.
pub fn list_height(area: Rect) -> usize {
    area.height.saturating_sub(LIST_TOP + 1) as usize
}

pub fn draw(f: &mut Frame, app: &App) {
    let t = theme(app);
    let area = f.area();
    let base = Style::new().fg(t.fg).bg(t.bg.unwrap_or(ratatui::style::Color::Reset));
    let dim = base.fg(t.dim);
    let bar = Style::new().bg(t.fg).fg(t.on_bar).add_modifier(Modifier::BOLD);
    f.render_widget(Block::new().style(base), area);

    // Quick-add input.
    let input_area = Rect { height: 1, ..area };
    let typing = matches!(app.focus, Focus::Input);
    let input_line = if !typing && app.input.is_empty() {
        Line::from(vec![Span::styled("+ ", base.fg(t.accent)), Span::styled("Quick add, goes to General", dim)])
    } else {
        Line::from(vec![Span::styled("+ ", base.fg(t.accent)), Span::styled(app.input.text(), base)])
    };
    f.render_widget(Paragraph::new(input_line), input_area);
    if typing {
        f.set_cursor_position(Position { x: area.x + 2 + app.input.cursor_col(), y: area.y });
    }

    // Lists. While dragging, the item leaves its place and a slot shows where it would land.
    let mut rows = rows(&app.doc, &app.view(app.dragged()));
    let mut drop_depth = 0;
    if let Gesture::Move { target_row, .. } = &app.gesture
        && !rows.is_empty()
    {
        let target = rows[(*target_row).min(rows.len() - 1)].clone();
        let (at, replace) = slot_position(&rows, (*target_row).min(rows.len() - 1));
        if replace {
            rows.remove(at);
        }
        let depth = target.drop.parent.len() as u16;
        drop_depth = depth;
        rows.insert(at, Row { kind: RowKind::Slot, at: None, depth, text: String::new(), offset: 0, done: false, marker: None, marker_col: 0, opens: false, ..target });
    }
    if app.doc.lists.is_empty() {
        // Below the "+ New list" row and its gap, which take the first two lines.
        let hint = Rect { y: area.y + LIST_TOP + 2, height: 1, ..area };
        let msg = if app.has_file() {
            "No todos in TODOS.md yet. Add one above, or start a list."
        } else {
            "No TODOS.md here yet. Add a to-do to start one."
        };
        f.render_widget(Paragraph::new(Span::styled(msg, dim)), hint);
    }
    let height = list_height(area);
    let block = current_block(app, &rows);
    let lit = base.fg(t.accent);
    for (i, row) in rows.iter().enumerate().skip(app.scroll).take(height) {
        let y = area.y + LIST_TOP + (i - app.scroll) as u16;
        let line_area = Rect { y, height: 1, ..area };
        // The line being written lights up its grip like the cursor item does.
        let on_cursor = (row.at.is_some() && row.at == app.cursor && matches!(app.focus, Focus::List | Focus::Edit { .. }))
            || (row.kind == RowKind::New && matches!(app.focus, Focus::New { .. }));
        let barred = on_cursor && t.cursor_bar && !matches!(app.gesture, Gesture::Move { .. });
        let row_style = if barred { bar } else { base };
        let mut spans = Vec::new();
        match row.kind {
            RowKind::Title => {
                let name_style = if app.doc.lists[row.list].is_done() { dim } else { base };
                spans.push(Span::styled("● ", base.fg(t.accent)));
                match &app.focus {
                    Focus::EditList { list, line } if *list == row.list => {
                        spans.push(Span::styled(line.text(), base.add_modifier(Modifier::BOLD)));
                        f.set_cursor_position(Position { x: area.x + 2 + line.cursor_col(), y });
                    }
                    _ => spans.push(Span::styled(row.text.clone(), name_style.add_modifier(Modifier::BOLD))),
                }
                if let Some(marker) = &row.marker {
                    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                    spans.push(Span::styled(" ".repeat((row.marker_col as usize).saturating_sub(used)), base));
                    spans.push(Span::styled(marker.clone(), base.fg(t.list_marker).add_modifier(Modifier::BOLD)));
                }
            }
            RowKind::NewList => match &app.focus {
                Focus::NewList { line } => {
                    spans.push(Span::styled("● ", base.fg(t.accent)));
                    spans.push(Span::styled(line.text(), base.add_modifier(Modifier::BOLD)));
                    f.set_cursor_position(Position { x: area.x + 2 + line.cursor_col(), y });
                }
                _ => {
                    spans.push(Span::styled("+ ", dim));
                    spans.push(Span::styled("New list", dim.add_modifier(Modifier::UNDERLINED)));
                }
            },
            RowKind::Empty => spans.push(Span::styled("  nothing here", dim.add_modifier(Modifier::ITALIC))),
            RowKind::Gap => {}
            RowKind::Slot => {
                let slot_style = if t.cursor_bar { dim } else { base.fg(t.accent) };
                spans.extend(guides(&rows, i, lit_guide(row, &block), dim, lit));
                let used = row.lead() as usize + 2;
                spans.push(Span::styled(
                    format!("  {}", "┄".repeat((area.width as usize).saturating_sub(used))),
                    slot_style,
                ));
            }
            RowKind::AddHere => {
                spans.push(Span::styled("    + ", dim));
                spans.push(Span::styled("Add a to-do", dim.add_modifier(Modifier::UNDERLINED)));
            }
            RowKind::Item | RowKind::New => {
                let guide_style = if barred { bar } else { dim };
                spans.extend(guides(&rows, i, lit_guide(row, &block), guide_style, if barred { bar } else { lit }));
                if row.is_first() {
                    let grip_style = if barred { bar } else if on_cursor { base.fg(t.accent) } else { dim };
                    spans.push(Span::styled("⠿ ", grip_style));
                    spans.push(Span::styled(if row.done { "[x] " } else { "[ ] " }, row_style));
                } else {
                    // A wrapped todo's later rows: text only, aligned under the first row's text.
                    if row.opens {
                        // The todo's own guide starts under its grip, so it runs unbroken
                        // through these wrapped rows down to its sub-items.
                        let own = block.as_ref().is_some_and(|(l, p)| *l == row.list && row.at.as_ref().is_some_and(|a| &a.path == p));
                        let (mark, style) = if own { ("┃", if barred { bar } else { lit }) } else { ("│", if barred { bar } else { dim }) };
                        spans.push(Span::styled(mark, style));
                        spans.push(Span::styled("     ", row_style));
                    } else {
                        spans.push(Span::styled("      ", row_style));
                    }
                }
                let mut text_style = row_style;
                if row.done {
                    text_style = text_style.add_modifier(Modifier::CROSSED_OUT);
                    if !barred {
                        text_style = text_style.fg(t.dim);
                    }
                }
                let editing = match &app.focus {
                    Focus::Edit { at, line } if row.at.as_ref() == Some(at) => Some(line),
                    Focus::New { line, .. } if row.kind == RowKind::New => Some(line),
                    _ => None,
                };
                if let Some(line) = editing {
                    // The text cursor sits on whichever row of the wrapped todo holds it.
                    let pos = line.cursor_col() as usize;
                    let len = row.text.chars().count();
                    let is_last = rows.get(i + 1).is_none_or(|next| next.is_first() || next.kind != row.kind);
                    if pos >= row.offset && (pos < row.offset + len || is_last) {
                        f.set_cursor_position(Position { x: area.x + row.text_start() + (pos - row.offset) as u16, y });
                    }
                }
                if let Some((from, to)) = app.selection_on(i, &rows) {
                    let chars: Vec<char> = row.text.chars().collect();
                    let part = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
                    spans.push(Span::styled(part(0, from), text_style));
                    // Bright text on the fill, even on the cursor bar, whose dark text would vanish.
                    spans.push(Span::styled(part(from, to), text_style.bg(t.panel).fg(t.fg)));
                    spans.push(Span::styled(part(to, chars.len()), text_style));
                } else {
                    spans.push(Span::styled(row.text.clone(), text_style));
                }
                if let Some(marker) = &row.marker {
                    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                    spans.push(Span::styled(" ".repeat((row.marker_col as usize).saturating_sub(used)), row_style));
                    spans.push(Span::styled(marker.clone(), if barred { bar } else { base.fg(t.accent).add_modifier(Modifier::BOLD) }));
                }
                if barred {
                    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
                    spans.push(Span::styled(" ".repeat((area.width as usize).saturating_sub(used)), bar));
                }
            }
        }
        f.render_widget(Paragraph::new(Line::from(spans)), line_area);
    }

    // The ghost: a solid copy of the dragged item that follows the pointer, indented to the
    // level it would land at, so nesting is visible before the drop.
    if let Gesture::Move { from, pointer_y, .. } = &app.gesture
        && let Some(item) = app.doc.item(from)
    {
        let y = (*pointer_y).clamp(area.y + LIST_TOP, area.bottom().saturating_sub(2));
        let ghost_style = if t.cursor_bar { bar } else { Style::new().bg(t.accent).fg(t.on_bar).add_modifier(Modifier::BOLD) };
        let lead = (SUB_INDENT * drop_depth).min(area.width);
        let text = format!("⠿ {} {}", if item.done { "[x]" } else { "[ ]" }, item.full_text());
        // Pad to the full width so the ghost covers the drop slot under it.
        let text = format!("{text:<width$}", width = (area.width - lead) as usize);
        let ghost = Rect { x: area.x + lead, y, width: area.width - lead, height: 1 };
        f.render_widget(Paragraph::new(text).style(ghost_style), ghost);
    }

    // Status line: the last message, or key hints.
    let status_area = Rect { y: area.bottom().saturating_sub(1), height: 1, ..area };
    let status = match (&app.status, &app.focus) {
        (Some(msg), _) => Span::styled(msg.clone(), base.fg(t.status)),
        (None, Focus::Input) => Span::styled("enter add · esc done", dim),
        (None, Focus::Edit { .. } | Focus::New { .. }) => Span::styled("enter next line · tab nest · shift+tab unnest · esc done", dim),
        (None, Focus::EditList { .. } | Focus::NewList { .. }) => Span::styled("enter save · esc done · clear an empty list's name to remove it", dim),
        (None, Focus::List) => Span::styled("click to write · drag to move · click [ ] to tick · ctrl+z undo", dim),
    };
    f.render_widget(Paragraph::new(Line::from(status)), status_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(text: &str, width: usize) -> Vec<String> {
        wrap(text, width).into_iter().map(|(_, p)| p).collect()
    }

    #[test]
    fn wraps_after_spaces_and_keeps_every_char() {
        let text = "Write the release notes for version two";
        let rows = wrap(text, 16);
        assert_eq!(pieces(text, 16), ["Write the ", "release notes ", "for version two"]);
        assert_eq!(rows.iter().map(|(_, p)| p.as_str()).collect::<String>(), text);
        assert_eq!(rows.iter().map(|(o, _)| *o).collect::<Vec<_>>(), [0, 10, 24]);
    }

    #[test]
    fn breaks_long_words_and_keeps_empty_todos() {
        assert_eq!(pieces("abcdefghijklmnopqrstuvwxyz", 12), ["abcdefghijkl", "mnopqrstuvwx", "yz"]);
        assert_eq!(pieces("", 20), [""]);
        assert_eq!(pieces("short", 20), ["short"]);
    }

    #[test]
    fn collapsed_todos_hide_their_sub_items() {
        let doc = Doc::parse("## A\n- [ ] p\n  - [ ] c1\n    - [ ] c11\n- [ ] q\n");
        let mut collapsed = HashSet::new();
        fn view(c: &HashSet<String>) -> View<'_> {
            View { skip: None, pending: None, editing: None, live: None, collapsed: c, width: 60 }
        }
        let texts = |rows: Vec<Row>| rows.into_iter().filter(|r| r.kind == RowKind::Item).map(|r| (r.text, r.marker)).collect::<Vec<_>>();
        assert_eq!(texts(rows(&doc, &view(&collapsed))), [
            ("p".into(), Some("⏷".into())),
            ("c1".into(), Some("⏷".into())),
            ("c11".into(), None),
            ("q".into(), None)
        ]);
        collapsed.insert(collapse_key(&doc, &ItemRef { list: 0, path: vec![0] }));
        assert_eq!(texts(rows(&doc, &view(&collapsed))), [("p".into(), Some("⏵ 2".into())), ("q".into(), None)]);
    }

    #[test]
    fn the_current_blocks_guide_lights_up_on_its_rows_only() {
        let doc = Doc::parse("## A\n- [ ] p\n  - [ ] c1\n    - [ ] c11\n  - [ ] c2\n- [ ] q\n  - [ ] q1\n");
        let collapsed = HashSet::new();
        let view = View { skip: None, pending: None, editing: None, live: None, collapsed: &collapsed, width: 60 };
        let rows = rows(&doc, &view);
        // The block p owns (a leaf like c2 under the cursor, or p itself): guide level 0.
        let block = Some((0, vec![0]));
        let lit: Vec<(String, Option<usize>)> =
            rows.iter().filter(|r| r.kind == RowKind::Item).map(|r| (r.text.clone(), lit_guide(r, &block))).collect();
        assert_eq!(lit, [
            ("p".into(), None),
            ("c1".into(), Some(0)),
            ("c11".into(), Some(0)),
            ("c2".into(), Some(0)),
            ("q".into(), None),
            ("q1".into(), None)
        ]);
    }

    #[test]
    fn guides_close_with_a_corner_on_the_last_row_of_their_block() {
        let doc = Doc::parse("## A\n- [ ] p\n  - [ ] c1\n    - [ ] g1\n  - [ ] c2\n    - [ ] g2\n- [ ] q\n");
        let collapsed = HashSet::new();
        let view = View { skip: None, pending: None, editing: None, live: None, collapsed: &collapsed, width: 60 };
        let rows = rows(&doc, &view);
        let drawn: Vec<String> = (0..rows.len())
            .filter(|&i| rows[i].kind == RowKind::Item)
            .map(|i| {
                let lead: String = guides(&rows, i, None, Style::new(), Style::new()).iter().map(|s| s.content.to_string()).collect();
                format!("{lead}{}", rows[i].text)
            })
            .collect();
        assert_eq!(drawn, ["p", "│ c1", "│ ╰ g1", "│ c2", "╰ ╰ g2", "q"]);
    }

    #[test]
    fn a_collapsed_list_shows_only_its_title_with_a_count() {
        let doc = Doc::parse("## A\n- [ ] a1\n  - [ ] a11\n- [ ] a2\n\n## B\n- [ ] b1\n");
        let mut collapsed = HashSet::new();
        collapsed.insert(list_collapse_key("A"));
        let view = View { skip: None, pending: None, editing: None, live: None, collapsed: &collapsed, width: 40 };
        let shown: Vec<(RowKind, String, Option<String>)> =
            rows(&doc, &view).into_iter().map(|r| (r.kind, r.text, r.marker)).collect();
        assert_eq!(shown[0], (RowKind::Title, "A".into(), Some("⏵ 3".into())));
        assert_eq!(shown[1].0, RowKind::Gap, "no todos, no add row");
        assert_eq!(shown[2], (RowKind::Title, "B".into(), Some("⏷".into())));
    }
}
