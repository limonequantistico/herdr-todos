//! Panel state and input: what the keyboard and mouse do.
//!
//! Writing works like a notes app: click an item's text to edit it, Return starts a new todo
//! on the line below, Return on an empty line stops. Every key shortcut is optional.

use std::collections::HashSet;
use std::io;
use std::path::Path;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::clipboard;
use crate::doc::{Doc, Entry, ItemRef, Place, items_in};
use crate::store::{Freshness, Store};
use crate::ui::{self, LIST_TOP, Row, RowKind, View, collapse_key};

/// How many changes undo remembers.
const HISTORY: usize = 200;

/// A one-line text editor: the quick-add input and the line being written in a list.
#[derive(Debug, Clone, Default)]
pub struct LineEdit {
    chars: Vec<char>,
    pos: usize,
}

pub enum EditOutcome {
    Typing,
    Submit,
    Cancel,
}

impl LineEdit {
    /// `text` with the cursor at char `pos` (clamped to the end).
    pub fn at(text: &str, pos: usize) -> Self {
        let chars: Vec<char> = text.chars().collect();
        LineEdit { pos: pos.min(chars.len()), chars }
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn cursor_col(&self) -> u16 {
        self.pos as u16
    }

    pub fn clear(&mut self) {
        *self = LineEdit::default();
    }

    fn set_pos(&mut self, pos: usize) {
        self.pos = pos.min(self.chars.len());
    }

    /// Start of the word before the cursor (Option+←).
    fn word_left(&self) -> usize {
        let mut p = self.pos;
        while p > 0 && self.chars[p - 1].is_whitespace() {
            p -= 1;
        }
        while p > 0 && !self.chars[p - 1].is_whitespace() {
            p -= 1;
        }
        p
    }

    /// End of the word after the cursor (Option+→).
    fn word_right(&self) -> usize {
        let mut p = self.pos;
        while p < self.chars.len() && self.chars[p].is_whitespace() {
            p += 1;
        }
        while p < self.chars.len() && !self.chars[p].is_whitespace() {
            p += 1;
        }
        p
    }

    /// Text keys, mac-style. Ghostty (and most macOS terminals) send Option+←/→ as Alt+B /
    /// Alt+F, Cmd+←/→ as Ctrl+A / Ctrl+E, and Cmd+Backspace as Ctrl+U; terminals with a
    /// modern keyboard protocol send the arrows with their modifiers instead. Both work.
    fn key(&mut self, key: KeyEvent) -> EditOutcome {
        let m = key.modifiers;
        let word = m.intersects(KeyModifiers::ALT | KeyModifiers::CONTROL);
        let line = m.contains(KeyModifiers::SUPER);
        match key.code {
            KeyCode::Enter => return EditOutcome::Submit,
            KeyCode::Esc => return EditOutcome::Cancel,
            KeyCode::Left if line => self.pos = 0,
            KeyCode::Right if line => self.pos = self.chars.len(),
            KeyCode::Left if word => self.pos = self.word_left(),
            KeyCode::Right if word => self.pos = self.word_right(),
            KeyCode::Char('b') if m.contains(KeyModifiers::ALT) => self.pos = self.word_left(),
            KeyCode::Char('f') if m.contains(KeyModifiers::ALT) => self.pos = self.word_right(),
            KeyCode::Char('a') if m.contains(KeyModifiers::CONTROL) => self.pos = 0,
            KeyCode::Char('e') if m.contains(KeyModifiers::CONTROL) => self.pos = self.chars.len(),
            KeyCode::Char('u') if m.contains(KeyModifiers::CONTROL) => {
                self.chars.drain(..self.pos);
                self.pos = 0;
            }
            KeyCode::Char('k') if m.contains(KeyModifiers::CONTROL) => self.chars.truncate(self.pos),
            KeyCode::Char('w') if m.contains(KeyModifiers::CONTROL) => self.delete_word(),
            KeyCode::Backspace if word => self.delete_word(),
            KeyCode::Backspace if line => {
                self.chars.drain(..self.pos);
                self.pos = 0;
            }
            // Only plain (or shifted) characters are text; anything with a modifier is a command.
            KeyCode::Char(c) if !m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) => {
                self.chars.insert(self.pos, c);
                self.pos += 1;
            }
            KeyCode::Backspace if self.pos > 0 => {
                self.pos -= 1;
                self.chars.remove(self.pos);
            }
            KeyCode::Delete if self.pos < self.chars.len() => {
                self.chars.remove(self.pos);
            }
            KeyCode::Left => self.pos = self.pos.saturating_sub(1),
            KeyCode::Right => self.pos = (self.pos + 1).min(self.chars.len()),
            KeyCode::Home => self.pos = 0,
            KeyCode::End => self.pos = self.chars.len(),
            _ => {}
        }
        EditOutcome::Typing
    }

    fn delete_word(&mut self) {
        let from = self.word_left();
        self.chars.drain(from..self.pos);
        self.pos = from;
    }
}

pub enum Focus {
    List,
    Input,
    /// Writing in an existing item.
    Edit { at: ItemRef, line: LineEdit },
    /// Writing a new todo at `place`; it joins the file once it has text.
    New { place: Place, line: LineEdit },
}

#[derive(Debug, Clone)]
pub enum Gesture {
    None,
    /// Dragging `from` (it started as a `Press` that moved). `target_row` indexes the rows
    /// laid out without the item.
    Move { from: ItemRef, pointer_y: u16, target_row: usize },
    /// Pressed on a todo. Moving the pointer turns it into a `Move`. Releasing in place on the
    /// text starts writing at char `pos`; on the grip (`pos: None`) it just selects the todo,
    /// so a drag that barely moves never opens an edit. `x`/`y` is where it started.
    Press { at: ItemRef, pos: Option<usize>, x: u16, y: u16 },
    /// Selecting text in the line being written. Ends are (row index, char offset in the row).
    Select { anchor: (usize, usize), extent: (usize, usize) },
}

pub struct App {
    pub doc: Doc,
    store: Store,
    pub cursor: Option<ItemRef>,
    pub focus: Focus,
    pub input: LineEdit,
    pub gesture: Gesture,
    pub status: Option<String>,
    pub phosphor: bool,
    pub scroll: usize,
    pub quit: bool,
    /// Todos whose sub-items are hidden, by `ui::collapse_key`. Kept for this session only.
    collapsed: HashSet<String>,
    /// The document before each change, for undo; and what undo took back, for redo. Both
    /// are dropped when the file changes on disk, so undo never reverts someone else's edit.
    history: Vec<Doc>,
    future: Vec<Doc>,
    /// Counts reloads of an outside change. Code that computed rows or places before a save
    /// compares it across the save: a reload in between makes those positions stale.
    reloads: u64,
    /// A change on disk that arrived mid-gesture or mid-writing, applied once it ends.
    reload_pending: bool,
    area: Rect,
}

impl App {
    pub fn open(dir: &Path) -> io::Result<Self> {
        let (store, doc) = Store::open(dir)?;
        let mut app = App {
            doc,
            store,
            cursor: None,
            focus: Focus::List,
            input: LineEdit::default(),
            gesture: Gesture::None,
            status: None,
            phosphor: false,
            scroll: 0,
            quit: false,
            collapsed: HashSet::new(),
            history: Vec::new(),
            future: Vec::new(),
            reloads: 0,
            reload_pending: false,
            area: Rect::default(),
        };
        app.fix_cursor();
        if app.cursor.is_none() {
            app.focus = Focus::Input;
        }
        Ok(app)
    }

    /// The TODOS.md being shown (after following a symlink).
    pub fn file(&self) -> &Path {
        self.store.path()
    }

    pub fn set_area(&mut self, area: Rect) {
        self.area = area;
        self.keep_visible();
    }

    pub fn dragged(&self) -> Option<&ItemRef> {
        match &self.gesture {
            Gesture::Move { from, .. } => Some(from),
            _ => None,
        }
    }

    pub fn pending(&self) -> Option<&Place> {
        match &self.focus {
            Focus::New { place, .. } => Some(place),
            _ => None,
        }
    }

    /// What the layout needs beyond the document. `skip` leaves out an item being dragged.
    pub fn view<'a>(&'a self, skip: Option<&'a ItemRef>) -> View<'a> {
        let (editing, live) = match &self.focus {
            Focus::Edit { at, line } => (Some(at), Some(line.text())),
            Focus::New { line, .. } => (None, Some(line.text())),
            _ => (None, None),
        };
        View { skip, pending: self.pending(), editing, live, collapsed: &self.collapsed, width: self.area.width }
    }

    /// The rows as currently shown (minus any drag).
    fn layout(&self) -> Vec<Row> {
        ui::rows(&self.doc, &self.view(None))
    }

    /// Show the sub-items of the todo at `at`.
    fn expand(&mut self, at: &ItemRef) {
        let key = collapse_key(&self.doc, at);
        self.collapsed.remove(&key);
    }

    fn writing(&self) -> bool {
        matches!(self.focus, Focus::Edit { .. } | Focus::New { .. })
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
    }

    fn fix_cursor(&mut self) {
        if self.cursor.as_ref().and_then(|at| self.doc.item(at)).is_none() {
            self.cursor = self.doc.all_items().into_iter().next();
        }
    }

    /// Scroll so the line being written, or else the cursor item, is on screen.
    fn keep_visible(&mut self) {
        let rows = self.layout();
        let height = ui::list_height(self.area).max(1);
        let focus_row = match &self.focus {
            Focus::Edit { at, .. } => rows.iter().position(|r| r.at.as_ref() == Some(at)),
            Focus::New { .. } => rows.iter().position(|r| r.kind == RowKind::New),
            _ => rows.iter().position(|r| r.at.is_some() && r.at == self.cursor),
        };
        if let Some(i) = focus_row {
            if i < self.scroll {
                self.scroll = i.saturating_sub(1);
            } else if i >= self.scroll + height {
                self.scroll = i + 1 - height;
            }
        }
        self.scroll = self.scroll.min(rows.len().saturating_sub(height));
    }

    /// Run one change against the file: refuse it if the file changed under us (and show the
    /// new content instead), otherwise apply it and write the file. `Err` means nothing was
    /// changed.
    fn apply(&mut self, op: impl FnOnce(&mut Doc) -> Option<ItemRef>) -> Result<Option<ItemRef>, ()> {
        match self.store.check() {
            Ok(Freshness::Unchanged) => {}
            Ok(Freshness::Changed(doc)) => {
                self.reloaded(doc);
                self.say("TODOS.md changed on disk — reloaded, try again");
                return Err(());
            }
            Err(e) => {
                self.say(format!("can't read TODOS.md: {e}"));
                return Err(());
            }
        }
        let before = self.doc.clone();
        let at = op(&mut self.doc);
        if let Err(e) = self.store.write(&self.doc) {
            self.doc = before;
            self.say(format!("can't write TODOS.md: {e}"));
            return Err(());
        }
        if self.doc != before {
            self.history.push(before);
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
            self.future.clear();
        }
        if at.is_some() {
            self.cursor = at.clone();
        }
        self.fix_cursor();
        self.keep_visible();
        Ok(at)
    }

    /// Text that couldn't be saved goes into the quick-add box instead of being lost.
    fn rescue(&mut self, text: String) {
        self.input = LineEdit::at(&text, usize::MAX);
        self.say("TODOS.md changed on disk — reloaded; your text is in the add box");
    }

    /// The file may have changed on disk.
    pub fn on_file_event(&mut self) {
        if !matches!(self.gesture, Gesture::None) || self.writing() {
            self.reload_pending = true;
            return;
        }
        self.reload_pending = false;
        match self.store.check() {
            Ok(Freshness::Changed(doc)) => self.reloaded(doc),
            Ok(Freshness::Unchanged) => {}
            Err(e) => self.say(format!("can't read TODOS.md: {e}")),
        }
    }

    /// Show the file as someone else left it.
    fn reloaded(&mut self, doc: Doc) {
        self.doc = doc;
        self.reloads += 1;
        self.history.clear();
        self.future.clear();
        self.fix_cursor();
        self.keep_visible();
    }

    /// Undo (`back`) or redo the last change to the file.
    fn travel(&mut self, back: bool) {
        let stack = if back { &mut self.history } else { &mut self.future };
        let Some(target) = stack.pop() else {
            return self.say(if back { "nothing to undo" } else { "nothing to redo" });
        };
        match self.store.check() {
            Ok(Freshness::Unchanged) => {}
            Ok(Freshness::Changed(doc)) => {
                self.reloaded(doc);
                return self.say("TODOS.md changed on disk — reloaded, nothing to undo");
            }
            Err(e) => return self.say(format!("can't read TODOS.md: {e}")),
        }
        if let Err(e) = self.store.write(&target) {
            let stack = if back { &mut self.history } else { &mut self.future };
            stack.push(target);
            return self.say(format!("can't write TODOS.md: {e}"));
        }
        let current = std::mem::replace(&mut self.doc, target);
        if back { self.future.push(current) } else { self.history.push(current) }
        self.fix_cursor();
        self.keep_visible();
        self.say(if back { "undone · ctrl+y to redo" } else { "redone" });
    }

    fn settle(&mut self) {
        if self.reload_pending {
            self.on_file_event();
        }
    }

    // ---- Writing ----------------------------------------------------------------------

    fn start_edit(&mut self, at: ItemRef, pos: usize) {
        if let Some(item) = self.doc.item(&at) {
            let line = LineEdit::at(&item.text, pos);
            self.cursor = Some(at.clone());
            self.focus = Focus::Edit { at, line };
            self.keep_visible();
        }
    }

    fn start_new(&mut self, place: Place) {
        self.focus = Focus::New { place, line: LineEdit::default() };
        self.keep_visible();
    }

    /// Save what's being written and stop writing. Returns the index (in the layout as it was
    /// shown) of a row that went away — an empty new line, or an item cleared to nothing — so
    /// a click or move aimed below it can be re-aimed.
    fn commit(&mut self) -> Option<usize> {
        let rows = self.layout();
        let gone = match std::mem::replace(&mut self.focus, Focus::List) {
            Focus::Edit { at, line } => {
                let text = line.text().trim().to_string();
                if text.is_empty() {
                    // Clearing an item deletes it (unless something is nested under it).
                    let mut removed = false;
                    let _ = self.apply(|doc| {
                        removed = doc.remove(&at);
                        None
                    });
                    removed.then(|| rows.iter().position(|r| r.at.as_ref() == Some(&at))).flatten()
                } else {
                    // An untouched line is never rewritten, even if trimming would change it.
                    let untouched = self.doc.item(&at).is_none_or(|item| item.text == line.text() || item.text == text);
                    if !untouched
                        && self
                            .apply(|doc| {
                                doc.edit(&at, &text);
                                Some(at.clone())
                            })
                            .is_err()
                    {
                        self.rescue(text);
                    }
                    None
                }
            }
            Focus::New { place, line } => {
                let text = line.text().trim().to_string();
                if text.is_empty() {
                    rows.iter().position(|r| r.kind == RowKind::New)
                } else {
                    if self.apply(|doc| doc.insert(&place, &text)).is_err() {
                        self.rescue(text);
                    }
                    None
                }
            }
            other => {
                self.focus = other;
                None
            }
        };
        self.settle();
        gone
    }

    /// Return while writing: save, then open a new line right below at the same level.
    /// Return on an empty line just stops.
    fn next_line(&mut self) {
        let below = match &self.focus {
            Focus::Edit { at, line } if !line.text().trim().is_empty() => {
                // A ticked item in Done gets no new line under it: new todos don't start done.
                let in_done = at.depth() == 0 && self.doc.lists[at.list].is_done();
                (!in_done).then(|| Place { list: at.list, parent: at.parent().to_vec(), slot: at.index() + 1 })
            }
            Focus::New { place, line } if !line.text().trim().is_empty() => Some(Place { slot: place.slot + 1, ..place.clone() }),
            _ => None,
        };
        let reloads = self.reloads;
        self.commit();
        // Only carry on if the save went through (a refused save leaves the text in the add box)
        // and nothing reloaded meanwhile (`below` would point into the old document).
        if let Some(place) = below
            && self.status.is_none()
            && self.reloads == reloads
        {
            self.start_new(place);
        }
    }

    /// Tab / Shift+Tab while writing: nest the line under the one above, or move it out.
    fn nest(&mut self, inward: bool) {
        match std::mem::replace(&mut self.focus, Focus::List) {
            Focus::Edit { at, line } => {
                let text = line.text().trim().to_string();
                let moved = self.apply(|doc| {
                    if !text.is_empty() {
                        doc.edit(&at, &text);
                    }
                    if inward { doc.indent(&at) } else { doc.outdent(&at) }
                });
                let at = match moved {
                    Ok(Some(new)) => new,
                    Ok(None) => at,
                    // The file changed on disk and was reloaded: `at` may now be another todo.
                    // Keep the text safe in the add box rather than writing on in a stale spot.
                    Err(()) => {
                        if !text.is_empty() {
                            self.rescue(text);
                        }
                        return self.keep_visible();
                    }
                };
                // Nesting under a collapsed todo opens it, so the line stays in view.
                if at.depth() > 0 {
                    self.expand(&ItemRef { list: at.list, path: at.parent().to_vec() });
                }
                self.focus = Focus::Edit { at, line };
            }
            Focus::New { place, line } => {
                let place = if inward { self.nested_under_above(&place) } else { self.moved_out(&place) };
                if !place.parent.is_empty() {
                    self.expand(&ItemRef { list: place.list, path: place.parent.clone() });
                }
                self.focus = Focus::New { place, line };
            }
            other => self.focus = other,
        }
        self.keep_visible();
    }

    /// The same spot, one level in: the end of the sub-items of the item just above.
    fn nested_under_above(&self, place: &Place) -> Place {
        let Some(container) = self.doc.container(place.list, &place.parent) else { return place.clone() };
        let Some(above) = items_in(container).filter(|&i| i < place.slot).last() else { return place.clone() };
        let Entry::Item(item) = &container[above] else { return place.clone() };
        let mut parent = place.parent.clone();
        parent.push(above);
        Place { list: place.list, parent, slot: item.children.len() }
    }

    /// The same spot, one level out: right after the parent.
    fn moved_out(&self, place: &Place) -> Place {
        match place.parent.split_last() {
            Some((&last, grand)) => Place { list: place.list, parent: grand.to_vec(), slot: last + 1 },
            None => place.clone(),
        }
    }

    /// ↑/↓ while writing (and Backspace on an empty line, going up): save, then write in the
    /// nearest item above or below.
    fn move_line(&mut self, dir: i32) {
        let rows = self.layout();
        let here = match &self.focus {
            Focus::Edit { at, .. } => rows.iter().position(|r| r.at.as_ref() == Some(at)),
            Focus::New { .. } => rows.iter().position(|r| r.kind == RowKind::New),
            _ => None,
        };
        let Some(here) = here else { return };
        let own = rows[here].at.clone();
        let other_item = |i: &usize| rows[*i].kind == RowKind::Item && (own.is_none() || rows[*i].at != own);
        let target = if dir < 0 {
            (0..here).rev().find(other_item)
        } else {
            (here + 1..rows.len()).find(other_item)
        };
        let reloads = self.reloads;
        let gone = self.commit();
        let Some(mut t) = target else { return };
        if self.reloads != reloads {
            return; // `target` indexes the layout from before the reload
        }
        if gone.is_some_and(|g| g < t) {
            t -= 1;
        }
        if let Some(at) = self.layout().get(t).and_then(|r| r.at.clone()) {
            self.start_edit(at, usize::MAX);
        }
    }

    // ---- Keys -------------------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            // Leaving saves, like everywhere else.
            self.commit();
            self.quit = true;
            return;
        }
        self.status = None;
        let (ctrl, cmd, shift) = (
            key.modifiers.contains(KeyModifiers::CONTROL),
            key.modifiers.contains(KeyModifiers::SUPER),
            key.modifiers.contains(KeyModifiers::SHIFT),
        );
        let z = matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z'));
        // Ctrl+Z / Ctrl+Y. Cmd+Z and Cmd+Shift+Z too, for terminals that pass them through
        // (Ghostty keeps them for its own undo by default).
        let undo = z && (ctrl || cmd) && !shift;
        let redo = (key.code == KeyCode::Char('y') && ctrl) || (z && cmd && shift);
        if undo && self.writing() {
            // First undo the typing in this line; once it's back as it was, undo file changes.
            let saved = match &self.focus {
                Focus::Edit { at, .. } => self.doc.item(at).map(|i| i.text.clone()).unwrap_or_default(),
                _ => String::new(),
            };
            if let Focus::Edit { line, .. } | Focus::New { line, .. } = &mut self.focus
                && line.text() != saved
            {
                *line = LineEdit::at(&saved, usize::MAX);
                return;
            }
            self.commit();
        }
        // Redo changes the document too: save the line first so its position can't go stale.
        if redo && self.writing() {
            self.commit();
        }
        if undo || redo {
            return self.travel(undo);
        }
        match &mut self.focus {
            Focus::Input => match self.input.key(key) {
                EditOutcome::Submit if !self.input.text().trim().is_empty() => {
                    let text = self.input.text().trim().to_string();
                    self.input.clear();
                    let _ = self.apply(|doc| Some(doc.add(&text)));
                }
                EditOutcome::Submit | EditOutcome::Cancel => self.focus = Focus::List,
                EditOutcome::Typing => {}
            },
            Focus::Edit { line, .. } | Focus::New { line, .. } => match key.code {
                KeyCode::Enter => self.next_line(),
                KeyCode::Esc => {
                    self.commit();
                }
                KeyCode::Tab => self.nest(true),
                KeyCode::BackTab => self.nest(false),
                KeyCode::Up => self.move_line(-1),
                KeyCode::Down => self.move_line(1),
                KeyCode::Backspace if line.is_empty() => self.move_line(-1),
                _ => {
                    line.key(key);
                }
            },
            Focus::List => self.list_key(key),
        }
    }

    /// Optional keyboard control while not writing.
    fn list_key(&mut self, key: KeyEvent) {
        // The todos on screen, so the cursor never lands inside a collapsed one.
        let refs: Vec<ItemRef> = self.layout().into_iter().filter(|r| r.kind == RowKind::Item && r.is_first()).filter_map(|r| r.at).collect();
        let pos = self.cursor.as_ref().and_then(|c| refs.iter().position(|r| r == c));
        let cursor = self.cursor.clone();
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('a') | KeyCode::Char('i') | KeyCode::Char('/') => self.focus = Focus::Input,
            KeyCode::Char('t') => self.phosphor ^= true,
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(p) = pos {
                    self.cursor = refs.get(p + 1).or(refs.get(p)).cloned();
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(p) = pos {
                    self.cursor = refs.get(p.saturating_sub(1)).cloned();
                }
            }
            KeyCode::Char('g') | KeyCode::Home => self.cursor = refs.first().cloned(),
            KeyCode::Char('G') | KeyCode::End => self.cursor = refs.last().cloned(),
            KeyCode::Char(' ') | KeyCode::Char('x') => {
                if let Some(at) = cursor {
                    let _ = self.apply(|doc| doc.toggle(&at));
                }
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                if let Some(at) = cursor {
                    self.start_edit(at, usize::MAX);
                }
            }
            KeyCode::Tab => {
                if let Some(at) = cursor {
                    let _ = self.apply(|doc| doc.indent(&at));
                }
            }
            KeyCode::BackTab => {
                if let Some(at) = cursor {
                    let _ = self.apply(|doc| doc.outdent(&at));
                }
            }
            KeyCode::Char('J') => self.shift(1),
            KeyCode::Char('K') => self.shift(-1),
            KeyCode::Esc => self.gesture = Gesture::None,
            _ => {}
        }
        self.keep_visible();
    }

    /// Move the cursor item one place down (`1`) or up (`-1`) among its siblings. Top-level
    /// items cross into the next or previous list at the ends; sub-items stop there.
    fn shift(&mut self, dir: i32) {
        let Some(at) = self.cursor.clone() else { return };
        let Some(siblings) = self.doc.container(at.list, at.parent()) else { return };
        let items: Vec<usize> = items_in(siblings).collect();
        let Some(i) = items.iter().position(|&e| e == at.index()) else { return };
        let here = |slot| Place { list: at.list, parent: at.parent().to_vec(), slot };
        let order = self.doc.display_order();
        let li = order.iter().position(|&l| l == at.list);
        let to = if dir > 0 {
            match items.get(i + 1) {
                Some(&next) => here(next + 1),
                None => match li.and_then(|li| order.get(li + 1)) {
                    Some(&l) if at.depth() == 0 => Place { list: l, parent: Vec::new(), slot: self.doc.lists[l].start_slot() },
                    _ => return,
                },
            }
        } else if i > 0 {
            here(items[i - 1])
        } else {
            match li.and_then(|li| li.checked_sub(1)).map(|p| order[p]) {
                Some(l) if at.depth() == 0 => Place { list: l, parent: Vec::new(), slot: self.doc.lists[l].end_slot() },
                _ => return,
            }
        };
        let _ = self.apply(|doc| doc.drop_item(&at, to));
    }

    // ---- Mouse ------------------------------------------------------------------------

    fn row_at(&self, y: u16) -> Option<usize> {
        let r = (y.checked_sub(self.area.y + LIST_TOP)? as usize) + self.scroll;
        (r < self.scroll + ui::list_height(self.area)).then_some(r)
    }

    /// Row index under the pointer, clamped into `rows`.
    fn clamp_row(&self, y: u16, len: usize) -> usize {
        let r = (y.saturating_sub(self.area.y + LIST_TOP) as usize) + self.scroll;
        r.min(len.saturating_sub(1))
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => self.mouse_down(m),
            MouseEventKind::Drag(MouseButton::Left) => self.mouse_drag(m),
            MouseEventKind::Up(MouseButton::Left) => self.mouse_up(),
            MouseEventKind::ScrollDown => {
                let max = self.layout().len().saturating_sub(ui::list_height(self.area));
                self.scroll = (self.scroll + 3).min(max);
            }
            MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
            _ => {}
        }
    }

    fn mouse_down(&mut self, m: MouseEvent) {
        self.status = None;
        let mut r = self.row_at(m.row);
        // A press on the line being written moves its text cursor and may start a selection.
        if self.writing()
            && let Some(ri) = r
            && let Some(row) = self.layout().get(ri).filter(|row| self.is_writing_row(row))
        {
            let col = m.column.saturating_sub(self.area.x);
            if col >= row.text_start() {
                let chr = (col - row.text_start()) as usize;
                let pos = row.offset + chr;
                if let Focus::Edit { line, .. } | Focus::New { line, .. } = &mut self.focus {
                    line.set_pos(pos);
                }
                self.gesture = Gesture::Select { anchor: (ri, chr), extent: (ri, chr) };
            }
            return;
        }
        // Clicking anywhere else saves what's being written first, like leaving a field. If
        // that made a row go away, re-aim the click at what is now under the pointer. If it
        // reloaded an outside change, the click was aimed at the old layout: drop it.
        let reloads = self.reloads;
        if self.writing()
            && let Some(gone) = self.commit()
            && let Some(row) = r.as_mut()
            && gone < *row
        {
            *row -= 1;
        }
        if self.reloads != reloads {
            return;
        }
        if m.row == self.area.y {
            self.focus = Focus::Input;
            return;
        }
        if matches!(self.focus, Focus::Input) {
            self.focus = Focus::List;
        }
        let rows = self.layout();
        let Some(r) = r.filter(|&r| r < rows.len()) else { return };
        let row = &rows[r];
        let col = m.column.saturating_sub(self.area.x);
        match row.kind {
            RowKind::AddHere => self.start_new(row.drop.clone()),
            RowKind::Item => {
                let Some(at) = row.at.clone() else { return };
                if col < row.lead() {
                    return; // the indent to the left of a sub-item
                }
                self.cursor = Some(at.clone());
                // The marker sits at the right edge; everything from just left of it counts.
                let on_marker = row.marker.is_some() && col + 1 >= row.marker_col;
                if on_marker {
                    let key = collapse_key(&self.doc, &at);
                    if !self.collapsed.remove(&key) {
                        self.collapsed.insert(key);
                    }
                } else if row.is_first() && col <= row.grip_end() {
                    // The grip: drag to move; a click only selects (the cursor is set above).
                    self.gesture = Gesture::Press { at, pos: None, x: m.column, y: m.row };
                } else if row.is_first() && col <= row.check_end() {
                    let _ = self.apply(|doc| doc.toggle(&at));
                } else if col >= row.text_start() {
                    let pos = row.offset + (col - row.text_start()) as usize;
                    self.gesture = Gesture::Press { at, pos: Some(pos), x: m.column, y: m.row };
                }
            }
            _ => {}
        }
    }

    /// Whether `row` shows (part of) the line being written.
    fn is_writing_row(&self, row: &Row) -> bool {
        match &self.focus {
            Focus::Edit { at, .. } => row.at.as_ref() == Some(at),
            Focus::New { .. } => row.kind == RowKind::New,
            _ => false,
        }
    }

    fn mouse_drag(&mut self, m: MouseEvent) {
        match self.gesture.clone() {
            // A press turns into a drag once the pointer leaves the row or travels two columns,
            // so a slightly shaky click still just starts writing.
            Gesture::Press { at, x, y, .. } if m.row != y || m.column.abs_diff(x) >= 2 => {
                let len = ui::rows(&self.doc, &self.view(Some(&at))).len();
                let target_row = self.clamp_row(m.row, len);
                self.gesture = Gesture::Move { from: at, pointer_y: m.row, target_row };
            }
            Gesture::Press { .. } => {}
            Gesture::Move { from, .. } => {
                let len = ui::rows(&self.doc, &self.view(Some(&from))).len();
                let target_row = self.clamp_row(m.row, len);
                self.gesture = Gesture::Move { from, pointer_y: m.row, target_row };
            }
            Gesture::Select { anchor, .. } => {
                let rows = self.layout();
                let r = self.clamp_row(m.row, rows.len());
                let col = m.column.saturating_sub(self.area.x);
                let chr = col.saturating_sub(rows[r].text_start()) as usize;
                self.gesture = Gesture::Select { anchor, extent: (r, chr) };
            }
            Gesture::None => {}
        }
    }

    fn mouse_up(&mut self) {
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::Move { from, target_row, .. } => {
                let rows = ui::rows(&self.doc, &self.view(Some(&from)));
                if let Some(row) = rows.get(target_row.min(rows.len().saturating_sub(1))) {
                    let to = row.drop.clone();
                    let unmoved = to.list == from.list
                        && to.parent == from.parent()
                        && (to.slot == from.index() || to.slot == from.index() + 1);
                    if !unmoved {
                        let _ = self.apply(|doc| doc.drop_item(&from, to));
                    }
                }
            }
            // A click on a todo's text (no drag): start writing right there.
            Gesture::Press { at, pos: Some(pos), .. } => self.start_edit(at, pos),
            Gesture::Press { pos: None, .. } => {}
            // A click inside the line being written already moved its cursor.
            Gesture::Select { anchor, extent } if anchor == extent => {}
            Gesture::Select { anchor, extent } => {
                let rows = self.layout();
                let text = selected_text(&rows, anchor, extent);
                if !text.is_empty() {
                    match clipboard::copy(&text) {
                        Ok(()) => self.say(format!("copied “{}”", text.replace('\n', " / "))),
                        Err(e) => self.say(format!("copy failed: {e}")),
                    }
                }
            }
            Gesture::None => {}
        }
        self.settle();
    }

    /// Char range of row `r` covered by the current selection.
    pub fn selection_on(&self, r: usize, rows: &[Row]) -> Option<(usize, usize)> {
        let Gesture::Select { anchor, extent } = self.gesture else { return None };
        if anchor == extent {
            return None;
        }
        let (s, e) = ordered(anchor, extent);
        (s.0..=e.0).contains(&r).then(|| span(rows, r, s, e))
    }
}

fn ordered(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if a <= b { (a, b) } else { (b, a) }
}

fn span(rows: &[Row], r: usize, s: (usize, usize), e: (usize, usize)) -> (usize, usize) {
    let len = rows.get(r).filter(|row| row.has_text()).map_or(0, |row| row.text.chars().count());
    let from = if r == s.0 { s.1 } else { 0 }.min(len);
    let to = if r == e.0 { e.1 + 1 } else { len }.min(len).max(from);
    (from, to)
}

/// The selected text. Rows of one wrapped todo join back into one line; different todos go
/// on separate lines.
fn selected_text(rows: &[Row], a: (usize, usize), b: (usize, usize)) -> String {
    let (s, e) = ordered(a, b);
    let mut out = String::new();
    for r in (s.0..=e.0).filter(|&r| rows.get(r).is_some_and(Row::has_text)) {
        if !out.is_empty() && rows[r].is_first() {
            out.push('\n');
        }
        let (from, to) = span(rows, r, s, e);
        out.extend(rows[r].text.chars().skip(from).take(to - from));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(line: &mut LineEdit, code: KeyCode, mods: KeyModifiers) {
        line.key(KeyEvent::new(code, mods));
    }

    #[test]
    fn mac_text_keys_as_ghostty_sends_them() {
        let none = KeyModifiers::NONE;
        let mut line = LineEdit::at("buy oat milk", usize::MAX);
        press(&mut line, KeyCode::Char('b'), KeyModifiers::ALT); // Option+←
        assert_eq!(line.pos, 8);
        press(&mut line, KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(line.pos, 4);
        press(&mut line, KeyCode::Char('f'), KeyModifiers::ALT); // Option+→
        assert_eq!(line.pos, 7);
        press(&mut line, KeyCode::Char('a'), KeyModifiers::CONTROL); // Cmd+←
        assert_eq!(line.pos, 0);
        press(&mut line, KeyCode::Char('e'), KeyModifiers::CONTROL); // Cmd+→
        assert_eq!(line.pos, 12);
        press(&mut line, KeyCode::Backspace, KeyModifiers::ALT); // Option+Backspace
        assert_eq!(line.text(), "buy oat ");
        press(&mut line, KeyCode::Char('u'), KeyModifiers::CONTROL); // Cmd+Backspace
        assert_eq!(line.text(), "");
        // Letters with Alt are commands, never typed.
        press(&mut line, KeyCode::Char('x'), KeyModifiers::ALT);
        press(&mut line, KeyCode::Char('Y'), KeyModifiers::SHIFT);
        press(&mut line, KeyCode::Char('o'), none);
        assert_eq!(line.text(), "Yo");
    }

    #[test]
    fn arrows_with_modifiers_from_modern_terminals() {
        let mut line = LineEdit::at("one two three", 0);
        press(&mut line, KeyCode::Right, KeyModifiers::ALT);
        assert_eq!(line.pos, 3);
        press(&mut line, KeyCode::Right, KeyModifiers::SUPER);
        assert_eq!(line.pos, 13);
        press(&mut line, KeyCode::Left, KeyModifiers::CONTROL);
        assert_eq!(line.pos, 8);
        press(&mut line, KeyCode::Left, KeyModifiers::SUPER);
        assert_eq!(line.pos, 0);
    }

    /// A panel on a real TODOS.md in a fresh temp folder.
    fn panel(name: &str, text: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("herdr-todos-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("TODOS.md"), text).unwrap();
        let app = App::open(&dir).unwrap();
        (app, dir.join("TODOS.md"))
    }

    fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
        app.on_key(KeyEvent::new(code, mods));
    }

    #[test]
    fn redo_while_writing_never_writes_over_another_todo() {
        let (mut app, file) = panel("redo", "## A\n- [ ] one\n- [ ] two\n");
        key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE); // tick "one"
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL); // undo
        app.start_edit(ItemRef::top(0, 0), usize::MAX); // writing in "one"
        key(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL); // redo: "one" back in Done
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## A\n- [ ] two\n\n## Done\n- [x] one\n");
    }

    #[test]
    fn opening_and_leaving_a_todo_never_rewrites_it() {
        let text = "## A\n* [X]  oddly spaced \n";
        let (mut app, file) = panel("untouched", text);
        app.start_edit(ItemRef::top(0, 0), 0);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    }

    #[test]
    fn ctrl_c_saves_the_line_being_written() {
        let (mut app, file) = panel("ctrlc", "## A\n- [ ] one\n");
        app.start_new(Place { list: 0, parent: Vec::new(), slot: 1 });
        for c in "two".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app.quit);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## A\n- [ ] one\n- [ ] two\n");
    }
}
