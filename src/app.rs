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
use crate::state::{Handoff, Prefs, State};
use crate::store::{Freshness, Store};
use crate::theme::LIST_COLORS;
use crate::ui::{self, LIST_TOP, MenuHit, Row, RowKind, View, collapse_key, list_collapse_key};

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
    /// Renaming list `list` (clearing an empty list's name removes it).
    EditList { list: usize, line: LineEdit },
    /// Naming a new list.
    NewList { line: LineEdit },
    /// The menu a list's dot opens: its colours, and delete.
    ListMenu { list: usize },
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
    /// Pressed on a list's name or dot. Moving the pointer turns it into a `MoveList`;
    /// releasing in place starts renaming at char `pos`, or on the dot (`pos: None`) opens the
    /// list's menu.
    PressList { list: usize, pos: Option<usize>, x: u16, y: u16 },
    /// Dragging list `from`. While it lasts only list titles show; `target_row` indexes them.
    MoveList { from: usize, pointer_y: u16, target_row: usize },
    /// Selecting text in the line being written. Ends are (row index, char offset in the row).
    Select { anchor: (usize, usize), extent: (usize, usize) },
}

pub struct App {
    pub doc: Doc,
    store: Store,
    pub cursor: Option<ItemRef>,
    pub focus: Focus,
    pub input: LineEdit,
    /// The first char the quick-add box shows, when its text is too long for it.
    pub input_scroll: usize,
    pub gesture: Gesture,
    pub status: Option<String>,
    pub phosphor: bool,
    /// Whether outside edits to TODOS.md show up on their own. Without a watcher (it can fail
    /// on network drives, or when Linux runs out of watches) `r` reloads by hand.
    pub watching: bool,
    pub scroll: usize,
    pub quit: bool,
    /// Todos and lists whose contents are hidden, by `ui::collapse_key`. Remembered per file
    /// in the plugin's state folder.
    collapsed: HashSet<String>,
    /// The document before each change, for undo; and what undo took back, for redo. Both
    /// are dropped when the file changes on disk, so undo never reverts someone else's edit.
    history: Vec<Doc>,
    future: Vec<Doc>,
    /// The list the last `commit` of a new list created, so Return can start writing in it.
    last_new_list: Option<usize>,
    /// Counts changes that move rows around under code that computed positions before a
    /// save: reloading an outside change, or removing a list. Compared across a save, a
    /// change in between makes those positions stale.
    reloads: u64,
    /// A change on disk that arrived mid-gesture or mid-writing, applied once it ends.
    reload_pending: bool,
    area: Rect,
    /// Where the theme and collapsed todos are remembered (inside herdr only).
    state: Option<State>,
    /// The prefs as last saved, to write them only when they change.
    saved: Prefs,
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
            input_scroll: 0,
            gesture: Gesture::None,
            status: None,
            phosphor: false,
            watching: true,
            scroll: 0,
            quit: false,
            collapsed: HashSet::new(),
            history: Vec::new(),
            future: Vec::new(),
            reloads: 0,
            last_new_list: None,
            reload_pending: false,
            area: Rect::default(),
            state: None,
            saved: Prefs::default(),
        };
        // Opens on the first todo, so the keyboard works straight away.
        app.cursor = app.doc.all_items().into_iter().next();
        if app.cursor.is_none() {
            app.focus = Focus::Input;
        }
        Ok(app)
    }

    /// Restore the theme and collapsed todos, and after a restart on a new build, what the
    /// old panel handed over: undo history, cursor and scroll.
    pub fn attach_state(&mut self, state: State, restarted: bool) {
        let prefs = state.load_prefs();
        self.phosphor = prefs.phosphor;
        self.collapsed = prefs.collapsed.clone();
        self.saved = prefs;
        if restarted && let Some(h) = state.take_handoff() {
            if h.text == self.doc.render() {
                self.history = h.history.iter().map(|t| Doc::parse(t)).collect();
                self.future = h.future.iter().map(|t| Doc::parse(t)).collect();
            }
            self.cursor = h.cursor;
            self.fix_cursor();
            self.scroll = h.scroll;
        }
        self.state = Some(state);
    }

    fn prefs(&self) -> Prefs {
        Prefs { phosphor: self.phosphor, collapsed: self.collapsed.clone() }
    }

    /// Write the theme and collapsed todos if they changed. Called after every event.
    pub fn save_prefs(&mut self) {
        let prefs = self.prefs();
        if let Some(state) = &self.state
            && prefs != self.saved
        {
            state.save_prefs(&prefs, prefs.phosphor != self.saved.phosphor);
            self.saved = prefs;
        }
    }

    /// Leave undo history, cursor and scroll for the new build this panel restarts on.
    pub fn hand_off(&self) {
        if let Some(state) = &self.state {
            state.save_handoff(&Handoff {
                text: self.doc.render(),
                history: self.history.iter().map(Doc::render).collect(),
                future: self.future.iter().map(Doc::render).collect(),
                cursor: self.cursor.clone(),
                scroll: self.scroll,
            });
        }
    }

    /// Whether TODOS.md exists here.
    pub fn has_file(&self) -> bool {
        self.store.exists()
    }

    /// The TODOS.md being shown (after following a symlink).
    pub fn file(&self) -> &Path {
        self.store.path()
    }

    /// Called before every frame. Only keeps the scroll in range: following the cursor here
    /// would undo every mouse-wheel scroll on the next frame. The cursor is followed when
    /// something actually moves it (`keep_visible` after keys, clicks and edits).
    pub fn set_area(&mut self, area: Rect) {
        let resized = area != self.area;
        self.area = area;
        if resized {
            self.keep_visible();
            self.fit_input();
        } else {
            let max = self.layout().len().saturating_sub(ui::list_height(area));
            self.scroll = self.scroll.min(max);
        }
    }

    /// Scroll the quick-add box so its cursor shows.
    fn fit_input(&mut self) {
        let width = ui::input_width(self.area.width);
        self.input_scroll = ui::input_window(&self.input.text(), self.input.cursor_col() as usize, self.input_scroll, width);
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
        let moving_list = match self.gesture {
            Gesture::MoveList { from, .. } => Some(from),
            _ => None,
        };
        View { skip, pending: self.pending(), editing, live, collapsed: &self.collapsed, moving_list, naming_list: matches!(self.focus, Focus::NewList { .. }), width: self.area.width }
    }

    /// The rows as currently shown (minus any drag).
    fn layout(&self) -> Vec<Row> {
        ui::rows(&self.doc, &self.view(None))
    }

    /// The open list menu: its list, and where it's drawn (only while the title is on screen).
    /// Drawing and clicks both read this.
    pub fn list_menu(&self) -> Option<(usize, Rect)> {
        let Focus::ListMenu { list } = self.focus else { return None };
        let r = self.layout().iter().position(|r| r.kind == RowKind::Title && r.list == list)?;
        let r = r.checked_sub(self.scroll).filter(|&r| r < ui::list_height(self.area))?;
        Some((list, ui::list_menu_rect(self.area, self.area.y + LIST_TOP + r as u16)))
    }

    /// Close the list menu: it points at a list by position, which a change can shift.
    fn close_menu(&mut self) {
        if matches!(self.focus, Focus::ListMenu { .. }) {
            self.focus = Focus::List;
        }
    }

    /// Show the sub-items of the todo at `at`.
    fn expand(&mut self, at: &ItemRef) {
        let key = collapse_key(&self.doc, at);
        self.collapsed.remove(&key);
    }

    /// Whether restarting now would lose nothing: no text being written or typed into the
    /// quick-add box, nothing being dragged.
    pub fn can_restart(&self) -> bool {
        !self.writing() && matches!(self.gesture, Gesture::None) && self.input.is_empty()
    }

    fn writing(&self) -> bool {
        matches!(self.focus, Focus::Edit { .. } | Focus::New { .. } | Focus::EditList { .. } | Focus::NewList { .. })
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.status = Some(msg.into());
    }

    /// Nothing selected is a valid state; only a cursor on a todo that's gone is dropped.
    fn fix_cursor(&mut self) {
        if self.cursor.as_ref().is_some_and(|at| self.doc.item(at).is_none()) {
            self.cursor = None;
        }
    }

    /// Scroll so the line being written, or else the cursor item, is on screen.
    fn keep_visible(&mut self) {
        let rows = self.layout();
        let height = ui::list_height(self.area).max(1);
        let is_focus = |r: &Row| match &self.focus {
            Focus::Edit { at, .. } => r.at.as_ref() == Some(at),
            Focus::New { .. } => r.kind == RowKind::New,
            Focus::EditList { list, .. } => r.kind == RowKind::Title && r.list == *list,
            Focus::NewList { .. } => r.kind == RowKind::NewList,
            _ => r.at.is_some() && r.at == self.cursor,
        };
        // All rows of the focused todo (a wrapped one has several) should be on screen.
        if let (Some(first), Some(last)) = (rows.iter().position(is_focus), rows.iter().rposition(is_focus)) {
            if first < self.scroll {
                self.scroll = first.saturating_sub(1);
            } else if last >= self.scroll + height {
                self.scroll = (last + 1 - height).min(first);
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
        self.fit_input();
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

    /// `r`: read the file again, for when nothing is watching it.
    fn reload(&mut self) {
        match self.store.check() {
            Ok(Freshness::Changed(doc)) => {
                self.reloaded(doc);
                self.say("reloaded TODOS.md");
            }
            Ok(Freshness::Unchanged) => self.say("TODOS.md is up to date"),
            Err(e) => self.say(format!("can't read TODOS.md: {e}")),
        }
    }

    /// Show the file as someone else left it.
    fn reloaded(&mut self, doc: Doc) {
        self.doc = doc;
        self.reloads += 1;
        self.close_menu();
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
        self.close_menu();
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
            let line = LineEdit::at(&item.full_text(), pos);
            self.cursor = Some(at.clone());
            self.focus = Focus::Edit { at, line };
            self.keep_visible();
        }
    }

    fn start_new(&mut self, place: Place) {
        // Writing into a collapsed list opens it, so the new line is visible.
        if let Some(list) = self.doc.lists.get(place.list) {
            self.collapsed.remove(&list_collapse_key(&list.name));
        }
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
                    let untouched = self.doc.item(&at).is_none_or(|item| item.full_text() == line.text() || item.full_text() == text);
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
            Focus::EditList { list, line } => {
                let name = line.text().trim().to_string();
                if name.is_empty() {
                    // Clearing a list's name removes it, but only if it's empty.
                    let mut removed = false;
                    let _ = self.apply(|doc| {
                        removed = doc.remove_list(list);
                        None
                    });
                    if removed {
                        self.reloads += 1; // rows below it moved up
                    } else {
                        self.say("a list with todos in it can't be removed");
                    }
                } else if self.doc.lists.get(list).is_some_and(|l| l.name != name || l.is_implicit()) {
                    let old = self.doc.lists[list].name.clone();
                    if self
                        .apply(|doc| {
                            doc.rename_list(list, &name);
                            None
                        })
                        .is_ok()
                    {
                        // Keep it collapsed under its new name, and mark the layout as moved.
                        if self.collapsed.remove(&list_collapse_key(&old)) {
                            self.collapsed.insert(list_collapse_key(&name));
                        }
                        self.reloads += 1;
                    }
                }
                None
            }
            Focus::NewList { line } => {
                let name = line.text().trim().to_string();
                if !name.is_empty() {
                    let mut created = None;
                    let _ = self.apply(|doc| {
                        created = Some(doc.add_list(&name));
                        None
                    });
                    if created.is_some() {
                        self.reloads += 1; // new rows above anything below it
                    }
                    self.last_new_list = created;
                    None
                } else {
                    // The name row goes away again (unless the panel is too narrow for the
                    // top-row button, where it stays as "+ New list").
                    rows.iter().position(|r| r.kind == RowKind::NewList).filter(|_| ui::new_list_button(self.area.width).is_some())
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
        if let Focus::NewList { .. } = self.focus {
            self.last_new_list = None;
            self.commit();
            // Straight into the new list's first todo.
            if let Some(list) = self.last_new_list.take() {
                let slot = self.doc.lists[list].start_slot();
                self.start_new(Place { list, parent: Vec::new(), slot });
            }
            return;
        }
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
        // A key mid-drag drops the drag: what it holds would go stale if the key changes the file.
        if matches!(self.gesture, Gesture::Press { .. } | Gesture::PressList { .. } | Gesture::Move { .. } | Gesture::MoveList { .. }) {
            self.gesture = Gesture::None;
            self.settle();
        }
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
                Focus::Edit { at, .. } => self.doc.item(at).map(|i| i.full_text()).unwrap_or_default(),
                Focus::EditList { list, .. } => self.doc.lists.get(*list).map(|l| l.name.clone()).unwrap_or_default(),
                _ => String::new(),
            };
            if let Focus::Edit { line, .. } | Focus::New { line, .. } | Focus::EditList { line, .. } | Focus::NewList { line } =
                &mut self.focus
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
                    if let Ok(Some(at)) = self.apply(|doc| Some(doc.add(&text))) {
                        // Show where it went, even if General was collapsed.
                        let name = self.doc.lists[at.list].name.clone();
                        self.collapsed.remove(&list_collapse_key(&name));
                        self.keep_visible();
                    }
                }
                EditOutcome::Submit | EditOutcome::Cancel => self.focus = Focus::List,
                EditOutcome::Typing => {}
            },
            Focus::EditList { line, .. } | Focus::NewList { line } => match key.code {
                KeyCode::Enter => self.next_line(),
                KeyCode::Esc => {
                    self.commit();
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {}
                _ => {
                    line.key(key);
                }
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
            Focus::ListMenu { list } => match key.code {
                KeyCode::Delete | KeyCode::Backspace => {
                    let list = *list;
                    self.delete_list(list);
                }
                KeyCode::Esc => self.focus = Focus::List,
                _ => {}
            },
            Focus::List => self.list_key(key),
        }
        self.fit_input();
        // Typing can grow a wrapped line past the bottom: keep it on screen.
        if self.writing() {
            self.keep_visible();
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
            KeyCode::Char('n') => self.new_line_from_cursor(cursor.as_ref()),
            KeyCode::Char('l') => self.focus = Focus::NewList { line: LineEdit::default() },
            KeyCode::Char('t') => self.phosphor ^= true,
            // Not mid-drag: the drag's rows were laid out from the file as it was.
            KeyCode::Char('r') | KeyCode::Char('R') if matches!(self.gesture, Gesture::None) => self.reload(),
            // With nothing selected, the arrows pick up from the top or the bottom.
            KeyCode::Char('j') | KeyCode::Down => {
                self.cursor = match pos {
                    Some(p) => refs.get(p + 1).or(refs.get(p)).cloned(),
                    None => refs.first().cloned(),
                };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.cursor = match pos {
                    Some(p) => refs.get(p.saturating_sub(1)).cloned(),
                    None => refs.last().cloned(),
                };
            }
            KeyCode::Char('g') | KeyCode::Home => self.cursor = refs.first().cloned(),
            KeyCode::Char('G') | KeyCode::End => self.cursor = refs.last().cloned(),
            KeyCode::Char(' ') | KeyCode::Char('x') if pos.is_some() => {
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
            // Delete the selected todo (Backspace is the Mac "delete" key; fn+delete is Delete).
            // Only a todo you can see: never one hidden in a collapsed list.
            KeyCode::Delete | KeyCode::Backspace if pos.is_some() => {
                if let Some(at) = cursor {
                    self.delete(&at);
                }
            }
            KeyCode::Esc => {
                self.gesture = Gesture::None;
                self.cursor = None;
            }
            _ => {}
        }
        self.keep_visible();
    }

    /// `n`: a new todo right below the selected one, at its level. With nothing selected (or a
    /// ticked todo in Done, where new todos don't go), at the end of the first open list; with
    /// no open list, the quick-add box.
    fn new_line_from_cursor(&mut self, cursor: Option<&ItemRef>) {
        if let Some(at) = cursor.filter(|at| at.depth() > 0 || !self.doc.lists[at.list].is_done()) {
            return self.start_new(Place { list: at.list, parent: at.parent().to_vec(), slot: at.index() + 1 });
        }
        match self.doc.display_order().into_iter().find(|&l| !self.doc.lists[l].is_done()) {
            Some(list) => self.start_new(Place { list, parent: Vec::new(), slot: self.doc.lists[list].end_slot() }),
            None => self.focus = Focus::Input,
        }
    }

    /// Delete a todo with everything under it, and say how to get it back.
    fn delete(&mut self, at: &ItemRef) {
        let mut gone = None;
        if self
            .apply(|doc| {
                let (item, next) = doc.delete(at)?;
                gone = Some(item);
                next
            })
            .is_ok()
            && let Some(item) = gone
        {
            let nested = item.children.iter().filter(|c| matches!(c, crate::doc::Entry::Item(_))).count();
            let text: String = item.full_text().chars().take(30).collect();
            let more = if nested > 0 { format!(" and {nested} sub-item{}", if nested == 1 { "" } else { "s" }) } else { String::new() };
            self.say(format!("deleted “{text}”{more} · ctrl+z to undo"));
        }
    }

    /// Delete a list with everything in it, and say how to get it back.
    fn delete_list(&mut self, list: usize) {
        self.focus = Focus::List;
        let mut gone = None;
        if self
            .apply(|doc| {
                gone = doc.delete_list(list);
                None
            })
            .is_ok()
            && let Some(list) = gone
        {
            self.reloads += 1; // rows below it moved up
            // The cursor counts lists by position, and they just shifted.
            self.cursor = None;
            let todos = ui::count_items(&list.entries);
            let more = if todos > 0 { format!(" and {todos} todo{}", if todos == 1 { "" } else { "s" }) } else { String::new() };
            self.say(format!("deleted list “{}”{more} · ctrl+z to undo", list.name));
        }
    }

    /// Set a list's colour from the menu (the first swatch is the default) and close it.
    fn color_list(&mut self, list: usize, color: usize) {
        self.focus = Focus::List;
        let name = LIST_COLORS.get(color).filter(|c| !c.is_empty()).copied();
        let _ = self.apply(|doc| {
            doc.set_list_color(list, name);
            None
        });
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

    /// The title a dragged list goes in above. Past the last one means the end, unless the
    /// last is Done: nothing goes below Done, so dropping on it already means the end.
    fn list_target(&self, y: u16) -> usize {
        let rows = self.layout();
        let r = (y.saturating_sub(self.area.y + LIST_TOP) as usize) + self.scroll;
        let done_last = rows.last().is_some_and(|row| self.doc.lists[row.list].is_done());
        r.min(if done_last { rows.len().saturating_sub(1) } else { rows.len() })
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
        // With the list menu open, a click picks from it; anywhere else just closes it.
        if let Focus::ListMenu { list } = self.focus {
            match self.list_menu().and_then(|(_, rect)| ui::list_menu_hit(rect, m.column, m.row)) {
                Some(MenuHit::Color(c)) => self.color_list(list, c),
                Some(MenuHit::Delete) => self.delete_list(list),
                None => self.focus = Focus::List,
            }
            return;
        }
        let mut r = self.row_at(m.row);
        // A press on the line being written moves its text cursor and may start a selection.
        if self.writing()
            && let Some(ri) = r
            && let Some(row) = self.layout().get(ri).filter(|row| self.is_writing_row(row))
        {
            let col = m.column.saturating_sub(self.area.x);
            if col >= row.text_start() {
                let chr = ui::col_to_char(&row.text, (col - row.text_start()) as usize);
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
            let col = m.column.saturating_sub(self.area.x);
            self.cursor = None;
            if ui::new_list_button(self.area.width).is_some_and(|b| col >= b) {
                self.focus = Focus::NewList { line: LineEdit::default() };
                self.keep_visible();
            } else {
                // Put the text cursor where the click landed, in the text as it was showing.
                let shown_from = if matches!(self.focus, Focus::Input) { self.input_scroll } else { 0 };
                if col >= 2 {
                    let shown = ui::input_view(&self.input.text(), shown_from, ui::input_width(self.area.width));
                    self.input.set_pos(shown_from + ui::col_to_char(&shown, (col - 2) as usize));
                }
                self.input_scroll = shown_from;
                self.focus = Focus::Input;
                self.fit_input();
            }
            return;
        }
        if matches!(self.focus, Focus::Input) {
            self.focus = Focus::List;
        }
        let rows = self.layout();
        // Below the last row: clicking empty space clears the selection.
        let Some(r) = r.filter(|&r| r < rows.len()) else {
            self.cursor = None;
            return;
        };
        let row = &rows[r];
        let col = m.column.saturating_sub(self.area.x);
        match row.kind {
            RowKind::AddHere => self.start_new(row.drop.clone()),
            // Only on a panel too narrow for the top-row button.
            RowKind::NewList => {
                self.focus = Focus::NewList { line: LineEdit::default() };
                self.keep_visible();
            }
            // The dot: drag to move the list, like a todo's grip; a click opens the list's menu
            // (colours, and delete). Done never moves, so its menu opens straight away.
            RowKind::Title if col <= 1 => {
                if self.doc.lists[row.list].is_done() {
                    self.cursor = None;
                    self.focus = Focus::ListMenu { list: row.list };
                } else {
                    self.gesture = Gesture::PressList { list: row.list, pos: None, x: m.column, y: m.row };
                }
            }
            // The marker at the right edge of a list title collapses or opens the list.
            RowKind::Title if row.marker.is_some() && col + 1 >= row.marker_col => {
                let key = list_collapse_key(&row.text);
                if !self.collapsed.remove(&key) {
                    self.collapsed.insert(key);
                }
            }
            // Click a list's name to rename it (Done keeps its name: it's what makes it Done).
            // Dragging it moves the list.
            RowKind::Title if !self.doc.lists[row.list].is_done() => {
                let pos = ui::col_to_char(&row.text, col.saturating_sub(2) as usize);
                self.gesture = Gesture::PressList { list: row.list, pos: Some(pos), x: m.column, y: m.row };
            }
            RowKind::Item => {
                let Some(at) = row.at.clone() else { return };
                if col < row.lead() {
                    self.cursor = None; // the indent to the left of a sub-item
                    return;
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
                    let pos = row.offset + ui::col_to_char(&row.text, (col - row.text_start()) as usize);
                    self.gesture = Gesture::Press { at, pos: Some(pos), x: m.column, y: m.row };
                }
            }
            // Blank rows between lists, the spacer, "nothing here": empty space.
            _ => self.cursor = None,
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
            Gesture::PressList { list, x, y, .. } if m.row != y || m.column.abs_diff(x) >= 2 => {
                self.gesture = Gesture::MoveList { from: list, pointer_y: m.row, target_row: 0 };
                // Only titles show from here on: start from the top so they all fit.
                self.scroll = 0;
                self.gesture = Gesture::MoveList { from: list, pointer_y: m.row, target_row: self.list_target(m.row) };
            }
            Gesture::PressList { .. } => {}
            Gesture::MoveList { from, .. } => {
                let target_row = self.list_target(m.row);
                self.gesture = Gesture::MoveList { from, pointer_y: m.row, target_row };
            }
            Gesture::Move { from, .. } => {
                let len = ui::rows(&self.doc, &self.view(Some(&from))).len();
                let target_row = self.clamp_row(m.row, len);
                self.gesture = Gesture::Move { from, pointer_y: m.row, target_row };
            }
            Gesture::Select { anchor, .. } => {
                let rows = self.layout();
                let r = self.clamp_row(m.row, rows.len());
                let col = m.column.saturating_sub(self.area.x);
                let chr = ui::col_to_char(&rows[r].text, col.saturating_sub(rows[r].text_start()) as usize);
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
            Gesture::MoveList { from, target_row, .. } => {
                // Rows laid out as during the drag: the other lists' titles, Done last.
                let view = View { moving_list: Some(from), ..self.view(None) };
                let rows = ui::rows(&self.doc, &view);
                let before = rows.get(target_row).map(|r| r.list).filter(|&l| !self.doc.lists[l].is_done());
                let cursor = self.cursor.clone();
                let _ = self.apply(|doc| {
                    let to = doc.move_list(from, before)?;
                    // Lists between the old and new place shift by one.
                    let moved = |i: usize| {
                        if i == from {
                            return to;
                        }
                        let i = if i > from { i - 1 } else { i };
                        if i >= to { i + 1 } else { i }
                    };
                    cursor.map(|c| ItemRef { list: moved(c.list), ..c })
                });
                self.keep_visible();
            }
            // A click on a list's name (no drag): rename it, starting where it was clicked.
            Gesture::PressList { list, pos: None, .. } => {
                self.cursor = None;
                self.focus = Focus::ListMenu { list };
            }
            Gesture::PressList { list, pos: Some(pos), .. } => {
                if let Some(l) = self.doc.lists.get(list) {
                    self.focus = Focus::EditList { list, line: LineEdit::at(&l.name, pos) };
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
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## A\n- [ ] two\n\n## Done\n- [x] one <!-- from: A -->\n");
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

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            key(app, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    #[test]
    fn new_list_then_its_first_todos_by_typing() {
        let (mut app, file) = panel("newlist", "### Work\n- a\n\n### Done\n- [x] d\n");
        app.focus = Focus::NewList { line: LineEdit::default() };
        type_text(&mut app, "Home");
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE); // creates the list, opens its first line
        type_text(&mut app, "milk");
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        type_text(&mut app, "bread");
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE); // empty line: stop
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "### Work\n- a\n\n### Home\n- milk\n- bread\n\n### Done\n- [x] d\n");
    }

    #[test]
    fn new_list_from_the_top_row_even_when_scrolled() {
        let todos: String = (0..30).map(|n| format!("- t{n}\n")).collect();
        let (mut app, file) = panel("newlist-top", &format!("### Work\n{todos}\n### Done\n- [x] d\n"));
        app.set_area(Rect { x: 0, y: 0, width: 40, height: 12 });
        assert!(!app.layout().iter().any(|r| r.kind == RowKind::NewList), "no name row until asked");
        app.scroll = 10;
        let click = |app: &mut App, column| {
            for kind in [MouseEventKind::Down(MouseButton::Left), MouseEventKind::Up(MouseButton::Left)] {
                app.on_mouse(MouseEvent { kind, column, row: 0, modifiers: KeyModifiers::NONE });
            }
        };
        click(&mut app, 4); // the quick-add box, not the button
        assert!(matches!(app.focus, Focus::Input));
        click(&mut app, 35); // "+ New list" at the right end
        assert!(matches!(app.focus, Focus::NewList { .. }));
        click(&mut app, 35);
        type_text(&mut app, "Home");
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(std::fs::read_to_string(&file).unwrap().contains("- t29\n\n### Home\n\n### Done"));
    }

    #[test]
    fn n_writes_below_the_selected_todo_and_l_starts_a_list() {
        let (mut app, file) = panel("keys", "### Work\n- a\n  - a1\n- b\n\n### Home\n- h\n\n### Done\n- [x] d\n");
        let enter = |app: &mut App, text: &str| {
            type_text(app, text);
            key(app, KeyCode::Esc, KeyModifiers::NONE);
        };
        // Nothing selected: the end of the first list.
        app.cursor = None;
        key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
        enter(&mut app, "c");
        // Below a selected sub-item, at its level.
        app.cursor = Some(ItemRef { list: 0, path: vec![0, 0] });
        key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
        enter(&mut app, "a2");
        key(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
        enter(&mut app, "Later");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "### Work\n- a\n  - a1\n  - a2\n- b\n- c\n\n### Home\n- h\n\n### Later\n\n### Done\n- [x] d\n"
        );
    }

    #[test]
    fn a_click_that_drops_an_unnamed_list_still_hits_its_target() {
        let (mut app, file) = panel("newlist-misaim", "### Work\n- a\n\n### Done\n- [x] d1\n- [x] d2\n");
        app.set_area(Rect { x: 0, y: 0, width: 40, height: 20 });
        key(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
        // Tick off d1's box as shown, with the name row still above it.
        let d1 = app.layout().iter().position(|r| r.text == "d1").unwrap();
        let y = LIST_TOP + d1 as u16;
        for kind in [MouseEventKind::Down(MouseButton::Left), MouseEventKind::Up(MouseButton::Left)] {
            app.on_mouse(MouseEvent { kind, column: 3, row: y, modifiers: KeyModifiers::NONE });
        }
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("- [x] d2") && !text.contains("- [x] d1"), "{text}");
    }

    #[test]
    fn renaming_and_removing_a_list() {
        let (mut app, file) = panel("rename", "## Work\n- a\n\n## Empty\n");
        app.focus = Focus::EditList { list: 0, line: LineEdit::at("Work", usize::MAX) };
        type_text(&mut app, "!");
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("## Work!\n"));
        app.focus = Focus::EditList { list: 1, line: LineEdit::at("Empty", usize::MAX) };
        key(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## Work!\n- a\n\n");
    }

    #[test]
    fn a_restart_keeps_theme_collapsed_lists_undo_and_cursor() {
        let (mut app, file) = panel("restart", "## A\n- [ ] a\n- [ ] b\n");
        let dir = file.parent().unwrap().join("state");
        app.attach_state(State::at(dir.clone(), &file), false);
        key(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        app.collapsed.insert(list_collapse_key("A"));
        app.cursor = Some(ItemRef::top(0, 1));
        key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE); // tick b
        app.save_prefs();
        app.hand_off();

        let mut next = App::open(file.parent().unwrap()).unwrap();
        next.attach_state(State::at(dir, &file), true);
        assert!(next.phosphor);
        assert!(next.collapsed.contains(&list_collapse_key("A")));
        key(&mut next, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## A\n- [ ] a\n- [ ] b\n", "undo survived");
    }

    #[test]
    fn r_reloads_an_outside_edit_the_watcher_missed() {
        let (mut app, file) = panel("reload", "## A\n- [ ] a\n");
        key(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(app.status.as_deref(), Some("TODOS.md is up to date"));
        std::fs::write(&file, "## A\n- [ ] a\n- [ ] b\n").unwrap();
        key(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(app.status.as_deref(), Some("reloaded TODOS.md"));
        assert!(app.doc.item(&ItemRef::top(0, 1)).is_some_and(|i| i.text == "b"));
    }

    #[test]
    fn a_wheel_scroll_is_not_undone_by_the_next_frame() {
        let many: String = (0..40).map(|i| format!("- [ ] todo {i}\n")).collect();
        let (mut app, _) = panel("scroll", &format!("## A\n{many}"));
        let area = Rect { x: 0, y: 0, width: 40, height: 12 };
        app.set_area(area);
        assert_eq!(app.scroll, 0);
        for _ in 0..3 {
            app.on_mouse(MouseEvent { kind: MouseEventKind::ScrollDown, column: 5, row: 5, modifiers: KeyModifiers::NONE });
            app.set_area(area); // the redraw after each event
        }
        assert_eq!(app.scroll, 9, "the cursor stayed on todo 0, the view still moved");
    }

    #[test]
    fn delete_key_removes_the_selected_todo_and_undo_brings_it_back() {
        let text = "## A\n- [ ] a\n- [ ] b\n  - [ ] b1\n";
        let (mut app, file) = panel("delete", text);
        app.cursor = Some(ItemRef::top(0, 1));
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## A\n- [ ] a\n");
        assert_eq!(app.status.as_deref(), Some("deleted “b” and 1 sub-item · ctrl+z to undo"));
        assert_eq!(app.cursor, Some(ItemRef::top(0, 0)));
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    }

    #[test]
    fn the_list_dot_opens_a_menu_to_colour_or_delete_the_list() {
        let text = "## A\n- [ ] a\n\n## B\n- [ ] b\n- [ ] c\n\n## Done\n";
        let (mut app, file) = panel("list-menu", text);
        app.set_area(Rect { x: 0, y: 0, width: 40, height: 20 });
        let click = |app: &mut App, column, row| {
            for kind in [MouseEventKind::Down(MouseButton::Left), MouseEventKind::Up(MouseButton::Left)] {
                app.on_mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE });
            }
        };
        // Rows: 2 "A", 3 a, 4 add, 5 gap, 6 "B". The menu opens under B's title, on rows
        // 7–9; its swatches sit on row 8 from column 2, two columns apart.
        click(&mut app, 0, 6);
        assert!(matches!(app.focus, Focus::ListMenu { list: 1 }));
        click(&mut app, 4, 8); // the second swatch: red
        assert!(matches!(app.focus, Focus::List));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## A\n- [ ] a\n\n## B <!-- color: red -->\n- [ ] b\n- [ ] c\n\n## Done\n");
        // A click outside only closes it.
        click(&mut app, 0, 6);
        click(&mut app, 30, 3);
        assert!(matches!(app.focus, Focus::List));
        // Delete takes the list and its todos; undo brings them back, colour and all.
        let coloured = std::fs::read_to_string(&file).unwrap();
        click(&mut app, 0, 6);
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## A\n- [ ] a\n\n## Done\n");
        assert_eq!(app.status.as_deref(), Some("deleted list “B” and 2 todos · ctrl+z to undo"));
        key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), coloured);
        // The first swatch puts the default back; "delete" in the menu deletes.
        click(&mut app, 0, 6);
        click(&mut app, 2, 8);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        click(&mut app, 0, 2);
        click(&mut app, 16, 4);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## B\n- [ ] b\n- [ ] c\n\n## Done\n");
    }

    #[test]
    fn dragging_a_list_title_moves_the_list_and_a_click_still_renames() {
        let text = "## A\n- [ ] a\n\n## B\n- [ ] b\n\n## Done\n";
        let (mut app, file) = panel("move-list", text);
        app.set_area(Rect { x: 0, y: 0, width: 40, height: 20 });
        let mouse = |app: &mut App, kind, column, row| app.on_mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE });
        app.cursor = Some(ItemRef::top(1, 0)); // "b"
        // Rows: 2 "A", 3 a, 4 add, 5 gap, 6 "B". Drag B up onto A's title.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 4, 6);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 4, 3);
        // While dragging only titles show: 2 "A", 3 "Done". Pointer on A: B goes above it.
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 4, 2);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 4, 2);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## B\n- [ ] b\n\n## A\n- [ ] a\n\n## Done\n");
        // The dot drags too: B (now on row 2) back below A.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 0, 2);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 0, 4);
        // Titles only: 2 "A", 3 "Done". On Done: the end, before Done.
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 0, 3);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 0, 3);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        assert!(matches!(app.focus, Focus::List), "a drag doesn't open the menu");
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 4, 6);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 4, 2);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 4, 2);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 4, 2);
        assert_eq!(app.cursor, Some(ItemRef::top(0, 0)), "still on b");
        // A click without moving renames.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 4, 2);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 4, 2);
        assert!(matches!(app.focus, Focus::EditList { list: 0, .. }));
        // Dropped on Done's title: the end, still before Done.
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 4, 2);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 4, 9);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 4, 9);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    }

    #[test]
    fn without_done_a_list_can_be_dragged_to_the_end() {
        let (mut app, file) = panel("move-list-end", "## A\n- [ ] a\n\n## B\n- [ ] b\n\n## C\n- [ ] c\n");
        app.set_area(Rect { x: 0, y: 0, width: 40, height: 20 });
        let mouse = |app: &mut App, kind, row| app.on_mouse(MouseEvent { kind, column: 4, row, modifiers: KeyModifiers::NONE });
        // Drag A's title; while dragging, titles show at 2 "B", 3 "C". Below C is the end.
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 2);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 4);
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 10);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 10);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "## B\n- [ ] b\n\n## C\n- [ ] c\n\n## A\n- [ ] a\n");
    }

    #[test]
    fn clicking_empty_space_or_esc_clears_the_selection() {
        let (mut app, _) = panel("deselect", "## A\n- [ ] a\n- [ ] b\n");
        app.set_area(Rect { x: 0, y: 0, width: 40, height: 20 });
        let click = |app: &mut App, row| {
            for kind in [MouseEventKind::Down(MouseButton::Left), MouseEventKind::Up(MouseButton::Left)] {
                app.on_mouse(MouseEvent { kind, column: 10, row, modifiers: KeyModifiers::NONE });
            }
        };
        assert_eq!(app.cursor, Some(ItemRef::top(0, 0)));
        click(&mut app, 18);
        assert_eq!(app.cursor, None);
        key(&mut app, KeyCode::Backspace, KeyModifiers::NONE); // nothing to delete
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(app.cursor, Some(ItemRef::top(0, 0)));
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.cursor, None);
    }
}
