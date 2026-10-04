//! The `TODOS.md` document: parsing, writing back, and the list operations.
//!
//! Format, read loosely so most hand-written TODOS.md files work as they are:
//! - Lists are headings at the level the file uses for them: `##` usually, `###` if that's the
//!   shallowest below the title, `#` when a file has several top-level headings.
//! - Items are bullets: `- [ ] text`, `- [x] text`, or plain `- text` (an open todo). Indented
//!   bullets are sub-items; indented text right under an item continues its text.
//! - Bullets before the first list heading form an unnamed list, shown as General.
//!
//! Every other line (notes, blank lines, other markdown) is kept exactly where it was. Items
//! the panel hasn't changed are written back byte for byte, so opening the panel never
//! reformats anyone's file.

pub const GENERAL: &str = "General";
pub const DONE: &str = "Done";
/// Hidden notes the panel keeps in `TODOS.md`, as trailing HTML comments: a ticked todo's
/// list (`<!-- from: Work -->`, so unticking can send it back) and a list's colour
/// (`## Work <!-- color: red -->`).
const FROM: &str = "from";
const COLOR: &str = "color";

#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    /// Lines before the first `## ` heading.
    pub preamble: Vec<String>,
    pub lists: Vec<List>,
    pub trailing_newline: bool,
    /// Whether the file uses Windows line endings, so writing keeps them.
    crlf: bool,
    /// The heading level of lists (2 for `##`). New lists are written at this level.
    level: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct List {
    pub name: String,
    /// The heading line as written, if unchanged.
    raw: Option<String>,
    pub entries: Vec<Entry>,
    /// The unnamed list of bullets before any list heading: it has no heading line.
    implicit: bool,
    /// The colour of the list's dot, written as `<!-- color: red -->` after its name.
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Item(Item),
    /// Any line that isn't an item, kept verbatim.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub done: bool,
    /// The text on the item's own line. `full_text` adds any continuation lines.
    pub text: String,
    pub children: Vec<Entry>,
    indent: String,
    bullet: char,
    /// Whether the line has a `[ ]` box; plain bullets don't until they're ticked.
    checkbox: bool,
    /// Indented lines right below the item that continue its text, kept as written.
    cont: Vec<String>,
    /// The list a ticked todo came from, written as `<!-- from: List -->` after its text.
    from: Option<String>,
    /// The line as written; dropped as soon as the item changes, so it's re-rendered.
    raw: Option<String>,
}

/// An item: its list, then entry indices from the list down through sub-items.
/// `path: [2]` is the list's third entry; `path: [2, 0]` that item's first sub-item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRef {
    pub list: usize,
    pub path: Vec<usize>,
}

impl ItemRef {
    #[cfg(test)]
    pub fn top(list: usize, entry: usize) -> Self {
        ItemRef { list, path: vec![entry] }
    }

    /// 0 for a top-level item, 1 for its sub-items, and so on.
    pub fn depth(&self) -> usize {
        self.path.len() - 1
    }

    pub fn parent(&self) -> &[usize] {
        &self.path[..self.path.len() - 1]
    }

    pub fn index(&self) -> usize {
        self.path[self.path.len() - 1]
    }
}

/// A spot to put an item: a list, the path of the item whose sub-items it joins (empty for
/// top level), and an entry index in that container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub list: usize,
    pub parent: Vec<usize>,
    pub slot: usize,
}

impl Item {
    pub fn new(text: &str) -> Self {
        Item {
            done: false,
            text: text.to_string(),
            children: Vec::new(),
            indent: String::new(),
            bullet: '-',
            checkbox: true,
            cont: Vec::new(),
            from: None,
            raw: None,
        }
    }

    /// The whole text, continuation lines included, as the panel shows it.
    pub fn full_text(&self) -> String {
        let mut text = self.text.clone();
        for line in &self.cont {
            text.push(' ');
            text.push_str(line.trim());
        }
        text
    }

    fn indent_width(&self) -> usize {
        indent_width(&self.indent)
    }

    /// Unticking also forgets the list it came from.
    fn set_done(&mut self, done: bool) {
        if self.done != done {
            self.done = done;
            self.raw = None;
        }
        if !done {
            self.set_from(None);
        }
    }

    fn set_from(&mut self, from: Option<String>) {
        if self.from != from {
            self.from = from;
            self.raw = None;
        }
    }

    /// Move the item (and everything under it) to a new indent, keeping relative nesting.
    fn reindent(&mut self, indent: &str) {
        if self.indent == indent {
            return;
        }
        let old = std::mem::replace(&mut self.indent, indent.to_string());
        self.raw = None;
        let shift = |line: &str| match line.strip_prefix(old.as_str()) {
            Some(rest) => format!("{indent}{rest}"),
            None => format!("{indent}  {}", line.trim_start()),
        };
        for line in &mut self.cont {
            *line = shift(line);
        }
        for child in &mut self.children {
            match child {
                Entry::Item(sub) => {
                    let sub_indent = shift(&sub.indent);
                    sub.reindent(&sub_indent);
                }
                Entry::Raw(line) if !line.trim().is_empty() => *line = shift(line),
                Entry::Raw(_) => {}
            }
        }
    }
}

/// Indices of the items among `entries` (skipping notes and blank lines).
pub fn items_in(entries: &[Entry]) -> impl Iterator<Item = usize> + '_ {
    entries.iter().enumerate().filter(|(_, e)| matches!(e, Entry::Item(_))).map(|(i, _)| i)
}

impl List {
    fn new(name: &str) -> Self {
        List { name: name.to_string(), raw: None, entries: Vec::new(), implicit: false, color: None }
    }

    pub fn is_done(&self) -> bool {
        self.name.eq_ignore_ascii_case(DONE)
    }

    /// Whether this is the unnamed list of bullets before any heading.
    pub fn is_implicit(&self) -> bool {
        self.implicit
    }

    #[cfg(test)]
    fn item(&self, entry: usize) -> Option<&Item> {
        match self.entries.get(entry) {
            Some(Entry::Item(item)) => Some(item),
            _ => None,
        }
    }

    pub fn item_entries(&self) -> impl Iterator<Item = usize> + '_ {
        items_in(&self.entries)
    }

    /// Where an item appended to this list goes: after the last item, before trailing notes
    /// or blank lines.
    pub fn end_slot(&self) -> usize {
        self.item_entries().last().map_or(0, |i| i + 1)
    }

    /// Where an item prepended to this list goes.
    pub fn start_slot(&self) -> usize {
        self.item_entries().next().unwrap_or(0)
    }
}

fn indent_width(indent: &str) -> usize {
    indent.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

struct Parsed {
    indent: String,
    bullet: char,
    checkbox: bool,
    done: bool,
    text: String,
}

/// `- [ ] text`, `- [x] text`, or a plain `- text`.
fn parse_item(line: &str) -> Option<Parsed> {
    let body = line.trim_start();
    let indent = line[..line.len() - body.len()].to_string();
    let mut chars = body.chars();
    let bullet = chars.next().filter(|c| matches!(c, '-' | '*' | '+'))?;
    let rest = chars.as_str().strip_prefix(' ')?;
    // A thematic break (`- - -`, `* * *`) is not a todo.
    if rest.chars().all(|c| c == bullet || c == ' ') {
        return None;
    }
    let boxed = |done, after: &str| Parsed { indent: indent.clone(), bullet, checkbox: true, done, text: after.to_string() };
    for (mark, done) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
        if let Some(after) = rest.strip_prefix(mark) {
            if after.is_empty() {
                return Some(boxed(done, ""));
            }
            if let Some(text) = after.strip_prefix(' ') {
                return Some(boxed(done, text));
            }
        }
    }
    Some(Parsed { indent, bullet, checkbox: false, done: false, text: rest.to_string() })
}

/// Split a trailing `<!-- key: value -->` note off a line's text.
fn split_note(text: &str, key: &str) -> (String, Option<String>) {
    let open = format!("<!-- {key}:");
    if let Some(body) = text.trim_end().strip_suffix("-->")
        && let Some(i) = body.rfind(&open)
    {
        let value = body[i + open.len()..].trim();
        if !value.is_empty() {
            return (body[..i].trim_end().to_string(), Some(value.to_string()));
        }
    }
    (text.to_string(), None)
}

/// `text` with a `<!-- key: value -->` note after it, if there's a value.
fn with_note(text: &str, key: &str, value: Option<&String>) -> String {
    match value {
        Some(value) => format!("{} <!-- {key}: {value} -->", text.trim_end()),
        None => text.trim_end().to_string(),
    }
}

/// The level of a markdown heading line (`## x` → 2).
fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    ((1..=6).contains(&hashes) && line[hashes..].starts_with(' ')).then_some(hashes)
}

/// Which heading level the file uses for lists: the shallowest below the title, or `#`
/// when there are several top-level headings and nothing deeper; `##` for a new file.
fn list_level(text: &str) -> usize {
    let levels: Vec<usize> = text.lines().filter_map(heading_level).collect();
    match levels.iter().filter(|&&l| l >= 2).min() {
        Some(&l) => l,
        None if levels.iter().filter(|&&l| l == 1).count() >= 2 => 1,
        None => 2,
    }
}

/// The item an indented text line continues: the deepest last item it's indented under,
/// as long as nothing (no sub-item, note or blank line) came in between.
fn continued(entries: &mut [Entry], width: usize) -> Option<&mut Item> {
    let Some(Entry::Item(last)) = entries.last_mut() else { return None };
    if width <= last.indent_width() {
        return None;
    }
    if last.children.is_empty() {
        return Some(last);
    }
    continued(&mut last.children, width)
}

/// Push `entry` into `entries`, nesting it under the last item if it's indented deeper.
fn place(entries: &mut Vec<Entry>, entry: Entry, width: usize) {
    if let Some(Entry::Item(last)) = entries.last_mut()
        && width > last.indent_width()
    {
        return place(&mut last.children, entry, width);
    }
    entries.push(entry);
}

fn first_item(entries: &[Entry]) -> Option<&Entry> {
    items_in(entries).next().map(|i| &entries[i])
}

impl Doc {
    pub fn empty() -> Self {
        Doc { preamble: Vec::new(), lists: Vec::new(), trailing_newline: true, crlf: false, level: 2 }
    }

    pub fn parse(text: &str) -> Self {
        let level = list_level(text);
        let mut doc = Doc { trailing_newline: text.is_empty() || text.ends_with('\n'), crlf: text.contains("\r\n"), level, ..Doc::empty() };
        // Blank lines wait for the next line to see where they belong: a blank line between
        // two sub-items ("loose" markdown lists) stays inside their parent, so the second
        // sub-item still nests.
        let mut blanks: Vec<Entry> = Vec::new();
        for line in text.lines() {
            if heading_level(line) == Some(level) {
                if let Some(list) = doc.lists.last_mut() {
                    list.entries.append(&mut blanks);
                }
                let (name, color) = split_note(line[level..].trim(), COLOR);
                doc.lists.push(List { name, raw: Some(line.to_string()), color, ..List::new("") });
                continue;
            }
            let item = parse_item(line);
            if doc.lists.is_empty() {
                if item.is_none() {
                    doc.preamble.push(line.to_string());
                    continue;
                }
                // Bullets before any list heading: an unnamed list, shown as General.
                doc.lists.push(List { implicit: true, ..List::new(GENERAL) });
            }
            let list = doc.lists.last_mut().expect("a list exists by now");
            let (entry, width) = match item {
                Some(p) => {
                    let width = indent_width(&p.indent);
                    let (text, from) = split_note(&p.text, FROM);
                    let item = Item {
                        done: p.done,
                        text,
                        children: Vec::new(),
                        indent: p.indent,
                        bullet: p.bullet,
                        checkbox: p.checkbox,
                        cont: Vec::new(),
                        from,
                        raw: Some(line.to_string()),
                    };
                    (Entry::Item(item), width)
                }
                None if line.trim().is_empty() => {
                    blanks.push(Entry::Raw(line.to_string()));
                    continue;
                }
                None => {
                    let width = indent_width(&line[..line.len() - line.trim_start().len()]);
                    if blanks.is_empty()
                        && heading_level(line.trim_start()).is_none()
                        && let Some(item) = continued(&mut list.entries, width)
                    {
                        item.cont.push(line.to_string());
                        continue;
                    }
                    (Entry::Raw(line.to_string()), width)
                }
            };
            for blank in blanks.drain(..) {
                place(&mut list.entries, blank, width);
            }
            place(&mut list.entries, entry, width);
        }
        if let Some(list) = doc.lists.last_mut() {
            list.entries.append(&mut blanks);
        }
        doc
    }

    pub fn render(&self) -> String {
        let mut lines: Vec<String> = self.preamble.clone();
        for list in &self.lists {
            if !list.implicit {
                let heading = || with_note(&format!("{} {}", "#".repeat(self.level), list.name), COLOR, list.color.as_ref());
                lines.push(list.raw.clone().unwrap_or_else(heading));
            }
            render_entries(&list.entries, &mut lines);
        }
        let newline = if self.crlf { "\r\n" } else { "\n" };
        let mut out = lines.join(newline);
        if self.trailing_newline && !out.is_empty() {
            out.push_str(newline);
        }
        out
    }

    pub fn item(&self, at: &ItemRef) -> Option<&Item> {
        match self.container(at.list, at.parent())?.get(at.index()) {
            Some(Entry::Item(item)) => Some(item),
            _ => None,
        }
    }

    fn item_mut(&mut self, at: &ItemRef) -> Option<&mut Item> {
        match self.container_mut(at.list, at.parent())?.get_mut(at.index()) {
            Some(Entry::Item(item)) => Some(item),
            _ => None,
        }
    }

    /// The entries holding an item's siblings: a list's entries, or an item's sub-items.
    pub fn container(&self, list: usize, parent: &[usize]) -> Option<&Vec<Entry>> {
        let mut entries = &self.lists.get(list)?.entries;
        for &i in parent {
            match entries.get(i) {
                Some(Entry::Item(item)) => entries = &item.children,
                _ => return None,
            }
        }
        Some(entries)
    }

    fn container_mut(&mut self, list: usize, parent: &[usize]) -> Option<&mut Vec<Entry>> {
        let mut entries = &mut self.lists.get_mut(list)?.entries;
        for &i in parent {
            match entries.get_mut(i) {
                Some(Entry::Item(item)) => entries = &mut item.children,
                _ => return None,
            }
        }
        Some(entries)
    }

    /// Every item in display order, sub-items right after their parent.
    pub fn all_items(&self) -> Vec<ItemRef> {
        fn walk(entries: &[Entry], list: usize, path: &mut Vec<usize>, out: &mut Vec<ItemRef>) {
            for i in items_in(entries) {
                path.push(i);
                out.push(ItemRef { list, path: path.clone() });
                if let Entry::Item(item) = &entries[i] {
                    walk(&item.children, list, path, out);
                }
                path.pop();
            }
        }
        let mut out = Vec::new();
        for list in self.display_order() {
            walk(&self.lists[list].entries, list, &mut Vec::new(), &mut out);
        }
        out
    }

    /// The indent a new arrival in this container should get: its siblings', else one level
    /// under the parent.
    fn indent_for(&self, list: usize, parent: &[usize]) -> String {
        let siblings = self.container(list, parent);
        if let Some(Entry::Item(sibling)) = siblings.and_then(|s| items_in(s).next().map(|i| &s[i])) {
            return sibling.indent.clone();
        }
        match parent.split_last() {
            None => String::new(),
            Some((&last, grand)) => match self.container(list, grand).and_then(|c| c.get(last)) {
                Some(Entry::Item(p)) => format!("{}  ", p.indent),
                _ => String::new(),
            },
        }
    }

    pub fn find_list(&self, name: &str) -> Option<usize> {
        self.lists.iter().position(|l| l.name.eq_ignore_ascii_case(name))
    }

    /// Lists in display order: as in the file, except Done always goes last.
    pub fn display_order(&self) -> Vec<usize> {
        let (mut order, done): (Vec<usize>, Vec<usize>) = (0..self.lists.len()).partition(|&i| !self.lists[i].is_done());
        order.extend(done);
        order
    }

    /// The General list, created at the top of the file if missing.
    fn general(&mut self) -> usize {
        if let Some(i) = self.find_list(GENERAL) {
            return i;
        }
        let mut list = List::new(GENERAL);
        if !self.lists.is_empty() {
            list.entries.push(Entry::Raw(String::new()));
        }
        // Keep a separating blank line between the preamble and the new heading.
        if self.preamble.last().is_some_and(|l| !l.trim().is_empty()) {
            self.preamble.push(String::new());
        }
        self.lists.insert(0, list);
        0
    }

    /// The Done list, created at the bottom of the file if missing.
    fn done(&mut self) -> usize {
        if let Some(i) = self.find_list(DONE) {
            return i;
        }
        match self.lists.last_mut() {
            Some(last) if !matches!(last.entries.last(), Some(Entry::Raw(l)) if l.trim().is_empty()) => {
                last.entries.push(Entry::Raw(String::new()))
            }
            None if self.preamble.last().is_some_and(|l| !l.trim().is_empty()) => self.preamble.push(String::new()),
            _ => {}
        }
        self.lists.push(List::new(DONE));
        self.lists.len() - 1
    }

    /// Add a new item at the end of General. Returns where it landed.
    pub fn add(&mut self, text: &str) -> ItemRef {
        let list = self.general();
        let slot = self.lists[list].end_slot();
        self.insert(&Place { list, parent: Vec::new(), slot }, text).expect("General exists")
    }

    /// A new item written the way its future siblings are (or, with none yet, the file's
    /// first open todo): same bullet, and a `[ ]` box only if they have one, so a plain-bullet
    /// file stays plain.
    fn new_item(&self, list: usize, parent: &[usize], text: &str) -> Item {
        let mut item = Item::new(text);
        item.indent = self.indent_for(list, parent);
        let sibling = self.container(list, parent).and_then(|c| first_item(c)).or_else(|| {
            self.lists.iter().filter(|l| !l.is_done()).find_map(|l| first_item(&l.entries))
        });
        if let Some(Entry::Item(s)) = sibling {
            item.bullet = s.bullet;
            item.checkbox = s.checkbox || s.done;
        }
        item
    }

    /// Replace an item's text. An item that continued over several lines becomes one line.
    pub fn edit(&mut self, at: &ItemRef, text: &str) {
        if let Some(item) = self.item_mut(at)
            && item.full_text() != text
        {
            item.text = text.to_string();
            item.cont.clear();
            item.raw = None;
        }
    }

    /// Tick an open top-level item (it moves to the top of Done) or untick a ticked one (from
    /// Done it goes back to the end of the list it came from, else General; elsewhere it stays
    /// put). Sub-items tick and untick in place, under their parent. Returns where the item
    /// ends up.
    pub fn toggle(&mut self, at: &ItemRef) -> Option<ItemRef> {
        let done = self.item(at)?.done;
        let in_done = self.lists[at.list].is_done();
        if at.depth() > 0 {
            self.item_mut(at)?.set_done(!done);
            return Some(at.clone());
        }
        if !done {
            self.item_mut(at)?.set_done(true);
            if in_done {
                return Some(at.clone());
            }
            self.remember_from(at);
            let to = self.done();
            let slot = self.lists[to].start_slot();
            return self.move_item(at, Place { list: to, parent: Vec::new(), slot });
        }
        let from = self.item(at)?.from.clone();
        self.item_mut(at)?.set_done(false);
        if !in_done {
            return Some(at.clone());
        }
        // Back to the list it came from, if that's still around.
        let origin = from.and_then(|name| self.find_list(&name)).filter(|&i| !self.lists[i].is_done());
        let (to, at) = match origin {
            Some(to) => (to, at.clone()),
            None => {
                let had_general = self.find_list(GENERAL).is_some();
                // Creating General inserts it at the top, shifting every other list down by one.
                let at = if had_general { at.clone() } else { ItemRef { list: at.list + 1, ..at.clone() } };
                (self.general(), at)
            }
        };
        let slot = self.lists[to].end_slot();
        self.move_item(&at, Place { list: to, parent: Vec::new(), slot })
    }

    /// Note on an item about to go into Done which list it's leaving. General is the
    /// fallback anyway, so it isn't written.
    fn remember_from(&mut self, at: &ItemRef) {
        let name = self.lists[at.list].name.clone();
        let from = (!self.lists[at.list].is_done() && !name.eq_ignore_ascii_case(GENERAL)).then_some(name);
        if let Some(item) = self.item_mut(at) {
            item.set_from(from);
        }
    }

    /// Drop an item at `to` (counted before the item is taken out). Dropping it at the top
    /// level of Done ticks it; dragging a top-level item out of Done unticks it.
    pub fn drop_item(&mut self, at: &ItemRef, to: Place) -> Option<ItemRef> {
        let into_done = to.parent.is_empty() && self.lists.get(to.list)?.is_done();
        let out_of_done = at.depth() == 0 && self.lists.get(at.list)?.is_done() && !into_done;
        if into_done && !self.lists[at.list].is_done() {
            self.remember_from(at);
        }
        let item = self.item_mut(at)?;
        if into_done {
            item.set_done(true);
        } else if out_of_done {
            item.set_done(false);
        }
        self.move_item(at, to)
    }

    /// Insert a new, open item at `at` (at the indent of its new siblings).
    pub fn insert(&mut self, at: &Place, text: &str) -> Option<ItemRef> {
        let item = self.new_item(at.list, &at.parent, text);
        let container = self.container_mut(at.list, &at.parent)?;
        let slot = at.slot.min(container.len());
        container.insert(slot, Entry::Item(item));
        let mut path = at.parent.clone();
        path.push(slot);
        Some(ItemRef { list: at.list, path })
    }

    /// Delete an item on purpose, with everything nested under it. Returns it, and where the
    /// cursor should go next: the item that slid into its place, else the one before it, else
    /// its parent.
    pub fn delete(&mut self, at: &ItemRef) -> Option<(Item, Option<ItemRef>)> {
        self.item(at)?;
        let container = self.container_mut(at.list, at.parent())?;
        let Entry::Item(item) = container.remove(at.index()) else { unreachable!("checked above") };
        let items: Vec<usize> = items_in(container).collect();
        let sibling = |i: usize| {
            let mut path = at.parent().to_vec();
            path.push(i);
            ItemRef { list: at.list, path }
        };
        let next = items
            .iter()
            .find(|&&i| i >= at.index())
            .or_else(|| items.iter().rev().find(|&&i| i < at.index()))
            .map(|&i| sibling(i))
            .or_else(|| (at.depth() > 0).then(|| ItemRef { list: at.list, path: at.parent().to_vec() }));
        Some((item, next))
    }

    /// Remove an item with nothing under it. Items with sub-items or notes are never removed
    /// this way, so clearing a parent's text can't take anything else with it.
    pub fn remove(&mut self, at: &ItemRef) -> bool {
        let bare = |item: &Item| item.children.iter().all(|c| matches!(c, Entry::Raw(l) if l.trim().is_empty()));
        if !self.item(at).is_some_and(bare) {
            return false;
        }
        self.container_mut(at.list, at.parent()).is_some_and(|c| {
            c.remove(at.index());
            true
        })
    }

    /// Add an empty list, after the others but before Done. Returns its index.
    pub fn add_list(&mut self, name: &str) -> usize {
        let at = self.lists.iter().position(List::is_done).unwrap_or(self.lists.len());
        let blank = |entries: &[Entry]| matches!(entries.last(), Some(Entry::Raw(l)) if l.trim().is_empty());
        match at.checked_sub(1) {
            Some(prev) if !blank(&self.lists[prev].entries) => self.lists[prev].entries.push(Entry::Raw(String::new())),
            None if self.preamble.last().is_some_and(|l| !l.trim().is_empty()) => self.preamble.push(String::new()),
            _ => {}
        }
        let mut list = List::new(name);
        if at < self.lists.len() {
            // Keep a blank line before the Done heading that now follows.
            list.entries.push(Entry::Raw(String::new()));
        }
        self.lists.insert(at, list);
        at
    }

    /// Rename a list. The unnamed list gets a real heading this way.
    pub fn rename_list(&mut self, list: usize, name: &str) {
        let Some(l) = self.lists.get_mut(list) else { return };
        if l.name == name && !l.implicit {
            return;
        }
        let old = std::mem::replace(&mut l.name, name.to_string());
        l.raw = None;
        l.implicit = false;
        // Ticked todos from this list still know their way back.
        let renamed = Some(name.to_string());
        for list in self.lists.iter_mut().filter(|l| l.is_done()) {
            for entry in &mut list.entries {
                if let Entry::Item(item) = entry
                    && item.from.as_ref().is_some_and(|f| f.eq_ignore_ascii_case(&old))
                {
                    item.set_from(renamed.clone());
                }
            }
        }
    }

    /// Remove a list, only if nothing but blank lines is in it.
    pub fn remove_list(&mut self, list: usize) -> bool {
        let empty = self.lists.get(list).is_some_and(|l| l.entries.iter().all(|e| matches!(e, Entry::Raw(x) if x.trim().is_empty())));
        if empty {
            self.lists.remove(list);
        }
        empty
    }

    /// Delete a list with everything in it. Returns it.
    pub fn delete_list(&mut self, list: usize) -> Option<List> {
        if list >= self.lists.len() {
            return None;
        }
        let gone = self.lists.remove(list);
        // The list above keeps its blank line before the next heading; at the end of the file
        // it would be left trailing.
        if list == self.lists.len()
            && let Some(last) = self.lists.last_mut()
        {
            while matches!(last.entries.last(), Some(Entry::Raw(l)) if l.trim().is_empty()) {
                last.entries.pop();
            }
        }
        Some(gone)
    }

    /// Set a list's colour (`None`: the default). The unnamed list gets a heading to hold it.
    pub fn set_list_color(&mut self, list: usize, color: Option<&str>) {
        if let Some(l) = self.lists.get_mut(list)
            && (l.color.as_deref() != color || (l.implicit && color.is_some()))
        {
            l.color = color.map(String::from);
            l.raw = None;
            l.implicit &= color.is_none();
        }
    }

    /// Move list `from` to just before list `before`, or after the last list that isn't Done
    /// (`None`). Done can't move: it always shows last. Returns where the list landed, or
    /// `None` if nothing moved. Blank lines are fixed up so a heading never ends up glued to
    /// the list above it, and the unnamed list gets a heading once it isn't first.
    pub fn move_list(&mut self, from: usize, before: Option<usize>) -> Option<usize> {
        if self.lists.get(from).is_none_or(List::is_done) || before == Some(from) {
            return None;
        }
        let ends_blank = |l: &List| matches!(l.entries.last(), Some(Entry::Raw(x)) if x.trim().is_empty());
        let was_last = from + 1 == self.lists.len();
        let list = self.lists.remove(from);
        let to = match before {
            Some(b) if b > from => b - 1,
            Some(b) => b,
            None => self.lists.iter().rposition(|l| !l.is_done()).map_or(0, |i| i + 1),
        };
        if to == from {
            self.lists.insert(from, list);
            return None;
        }
        self.lists.insert(to, list);
        let last = self.lists.len() - 1;
        if to < last && !ends_blank(&self.lists[to]) {
            self.lists[to].entries.push(Entry::Raw(String::new()));
        }
        // The list that used to end the file now has a heading after it.
        if to == last && ends_blank(&self.lists[to]) {
            self.lists[to].entries.pop();
        }
        if was_last && to != last && ends_blank(&self.lists[last]) {
            self.lists[last].entries.pop();
        }
        match to.checked_sub(1) {
            Some(prev) if !ends_blank(&self.lists[prev]) => self.lists[prev].entries.push(Entry::Raw(String::new())),
            None if self.preamble.last().is_some_and(|l| !l.trim().is_empty()) => self.preamble.push(String::new()),
            _ => {}
        }
        for (i, l) in self.lists.iter_mut().enumerate() {
            if l.implicit && i > 0 {
                l.implicit = false;
                l.raw = None;
            }
        }
        Some(to)
    }

    /// Make an item the last sub-item of the item above it.
    pub fn indent(&mut self, at: &ItemRef) -> Option<ItemRef> {
        let siblings = self.container(at.list, at.parent())?;
        let above = items_in(siblings).take_while(|&i| i < at.index()).last()?;
        let Entry::Item(above_item) = &siblings[above] else { return None };
        let slot = above_item.children.len();
        let mut parent = at.parent().to_vec();
        parent.push(above);
        self.move_item(at, Place { list: at.list, parent, slot })
    }

    /// Move a sub-item out one level, right after its parent.
    pub fn outdent(&mut self, at: &ItemRef) -> Option<ItemRef> {
        let (&parent_index, grand) = at.parent().split_last()?;
        self.move_item(at, Place { list: at.list, parent: grand.to_vec(), slot: parent_index + 1 })
    }

    fn move_item(&mut self, at: &ItemRef, to: Place) -> Option<ItemRef> {
        self.item(at)?;
        // An item can't go inside itself.
        if to.list == at.list && to.parent.starts_with(&at.path) {
            return None;
        }
        self.container(to.list, &to.parent)?;
        let Entry::Item(mut item) = self.container_mut(at.list, at.parent())?.remove(at.index()) else {
            unreachable!("checked above that `at` is an item")
        };
        // Taking the item out shifts later siblings up by one; fix the destination to match.
        let mut to = to;
        let d = at.depth();
        if to.list == at.list && to.parent.len() > d && to.parent[..d] == at.path[..d] && to.parent[d] > at.path[d] {
            to.parent[d] -= 1;
        }
        if to.list == at.list && to.parent == at.parent() && to.slot > at.index() {
            to.slot -= 1;
        }
        let indent = self.indent_for(to.list, &to.parent);
        item.reindent(&indent);
        let container = self.container_mut(to.list, &to.parent)?;
        let slot = to.slot.min(container.len());
        container.insert(slot, Entry::Item(item));
        let mut path = to.parent;
        path.push(slot);
        Some(ItemRef { list: to.list, path })
    }
}

fn render_entries(entries: &[Entry], lines: &mut Vec<String>) {
    for entry in entries {
        match entry {
            Entry::Raw(line) => lines.push(line.clone()),
            Entry::Item(item) => {
                lines.push(item.raw.clone().unwrap_or_else(|| {
                    let line = if item.checkbox || item.done {
                        let mark = if item.done { 'x' } else { ' ' };
                        format!("{}{} [{mark}] {}", item.indent, item.bullet, item.text)
                    } else {
                        format!("{}{} {}", item.indent, item.bullet, item.text)
                    };
                    with_note(&line, FROM, item.from.as_ref())
                }));
                lines.extend(item.cont.iter().cloned());
                render_entries(&item.children, lines);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top_level(list: usize, slot: usize) -> Place {
        Place { list, parent: Vec::new(), slot }
    }

    fn sub(list: usize, path: &[usize]) -> ItemRef {
        ItemRef { list, path: path.to_vec() }
    }

    const SAMPLE: &str = "\
# My project todos

Some notes up here.

## Work
- [ ] Ship v0.2
  - [ ] Write release notes
  - [x] Fix login bug
* [X]  oddly spaced
- [ ] Reply to Marco
  a note under Marco

## Done
- [x] Old thing
";

    #[test]
    fn round_trips_unchanged() {
        assert_eq!(Doc::parse(SAMPLE).render(), SAMPLE);
        let no_newline = "## A\n- [ ] x";
        assert_eq!(Doc::parse(no_newline).render(), no_newline);
        assert_eq!(Doc::parse("").render(), "");
    }

    #[test]
    fn parses_lists_items_and_sub_items() {
        let doc = Doc::parse(SAMPLE);
        assert_eq!(doc.lists.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Work", "Done"]);
        let work = &doc.lists[0];
        assert_eq!(work.item_entries().count(), 3);
        let ship = work.item(0).unwrap();
        assert_eq!(ship.text, "Ship v0.2");
        assert_eq!(ship.children.len(), 2);
        let odd = work.item(1).unwrap();
        assert!(odd.done);
        assert_eq!(odd.text, " oddly spaced");
        let marco = work.item(2).unwrap();
        // An indented line right under a todo continues its text.
        assert!(marco.children.is_empty());
        assert_eq!(marco.full_text(), "Reply to Marco a note under Marco");
    }

    #[test]
    fn which_lines_are_items() {
        let parsed = |line| parse_item(line).map(|p| (p.checkbox, p.done, p.text));
        for line in ["-[ ] x", "1. [ ] x", "- - -", "* * *", "text", ""] {
            assert!(parsed(line).is_none(), "{line:?}");
        }
        assert_eq!(parsed("- [ ]"), Some((true, false, String::new())));
        assert_eq!(parsed("* [X] done"), Some((true, true, "done".into())));
        // Anything else after a bullet is a plain todo, text as written.
        assert_eq!(parsed("- plain todo"), Some((false, false, "plain todo".into())));
        assert_eq!(parsed("- [y] x"), Some((false, false, "[y] x".into())));
        assert_eq!(parsed("- [ ]x"), Some((false, false, "[ ]x".into())));
    }

    #[test]
    fn add_creates_file_structure() {
        let mut doc = Doc::parse("");
        let at = doc.add("Buy milk");
        assert_eq!(at, ItemRef::top(0, 0));
        assert_eq!(doc.render(), "## General\n- [ ] Buy milk\n");
        doc.add("Call mum");
        assert_eq!(doc.render(), "## General\n- [ ] Buy milk\n- [ ] Call mum\n");
    }

    #[test]
    fn add_puts_general_first_and_keeps_spacing() {
        let mut doc = Doc::parse("# Title\n## Work\n- [ ] a\n");
        doc.add("new");
        assert_eq!(doc.render(), "# Title\n\n## General\n- [ ] new\n\n## Work\n- [ ] a\n");
    }

    #[test]
    fn add_goes_after_last_item_not_after_trailing_blank() {
        let mut doc = Doc::parse("## General\n- [ ] a\n\n## Work\n");
        doc.add("b");
        assert_eq!(doc.render(), "## General\n- [ ] a\n- [ ] b\n\n## Work\n");
    }

    #[test]
    fn tick_moves_to_top_of_done_with_sub_items() {
        let mut doc = Doc::parse(SAMPLE);
        let at = doc.toggle(&ItemRef::top(0, 0)).unwrap();
        assert_eq!(at, ItemRef::top(1, 0));
        let out = doc.render();
        assert!(out.contains("## Done\n- [x] Ship v0.2 <!-- from: Work -->\n  - [ ] Write release notes\n  - [x] Fix login bug\n- [x] Old thing\n"), "{out}");
        assert!(out.contains("## Work\n* [X]  oddly spaced\n"), "{out}");
    }

    #[test]
    fn tick_creates_done_at_the_bottom() {
        let mut doc = Doc::parse("## General\n- [ ] a\n- [ ] b\n");
        doc.toggle(&ItemRef::top(0, 1));
        assert_eq!(doc.render(), "## General\n- [ ] a\n\n## Done\n- [x] b\n");
    }

    #[test]
    fn untick_in_done_returns_to_end_of_general() {
        let mut doc = Doc::parse("## General\n- [ ] a\n\n## Done\n- [x] b\n");
        let at = doc.toggle(&ItemRef::top(1, 0)).unwrap();
        assert_eq!(at, ItemRef::top(0, 1));
        assert_eq!(doc.render(), "## General\n- [ ] a\n- [ ] b\n\n## Done\n");
    }

    #[test]
    fn untick_in_done_returns_to_the_list_it_came_from() {
        let mut doc = Doc::parse("## General\n- [ ] a\n\n## Work\n- [ ] b\n- [ ] c\n");
        let at = doc.toggle(&ItemRef::top(1, 0)).unwrap();
        assert_eq!(doc.render(), "## General\n- [ ] a\n\n## Work\n- [ ] c\n\n## Done\n- [x] b <!-- from: Work -->\n");
        // The note survives a save and reload, and isn't part of the text.
        let mut doc = Doc::parse(&doc.render());
        assert_eq!(doc.item(&at).unwrap().text, "b");
        assert_eq!(doc.toggle(&at), Some(ItemRef::top(1, 1)));
        assert_eq!(doc.render(), "## General\n- [ ] a\n\n## Work\n- [ ] c\n- [ ] b\n\n## Done\n");
    }

    #[test]
    fn untick_goes_to_general_when_its_list_is_gone() {
        let mut doc = Doc::parse("## General\n- [ ] a\n\n## Done\n- [x] b <!-- from: Gone -->\n");
        assert_eq!(doc.toggle(&ItemRef::top(1, 0)), Some(ItemRef::top(0, 1)));
        assert_eq!(doc.render(), "## General\n- [ ] a\n- [ ] b\n\n## Done\n");
    }

    #[test]
    fn dragging_into_done_remembers_the_list_and_renaming_follows() {
        let mut doc = Doc::parse("## Work\n- [ ] a\n\n## Done\n");
        doc.drop_item(&ItemRef::top(0, 0), top_level(1, 0));
        assert_eq!(doc.render(), "## Work\n\n## Done\n- [x] a <!-- from: Work -->\n");
        doc.rename_list(0, "Job");
        assert_eq!(doc.render(), "## Job\n\n## Done\n- [x] a <!-- from: Job -->\n");
        // Dragged back out, it's open again and forgets.
        doc.drop_item(&ItemRef::top(1, 0), top_level(0, 0));
        assert_eq!(doc.render(), "## Job\n- [ ] a\n\n## Done\n");
    }

    #[test]
    fn untick_in_done_creates_general_when_missing() {
        let mut doc = Doc::parse("## Work\n- [ ] a\n\n## Done\n- [x] b\n");
        let at = doc.toggle(&ItemRef::top(1, 0)).unwrap();
        assert_eq!(at, ItemRef::top(0, 0));
        assert_eq!(doc.render(), "## General\n- [ ] b\n\n## Work\n- [ ] a\n\n## Done\n");
    }

    #[test]
    fn untick_outside_done_stays_put() {
        let mut doc = Doc::parse("## Work\n- [x] a\n");
        assert_eq!(doc.toggle(&ItemRef::top(0, 0)), Some(ItemRef::top(0, 0)));
        assert_eq!(doc.render(), "## Work\n- [ ] a\n");
    }

    #[test]
    fn drop_reorders_within_a_list() {
        let mut doc = Doc::parse("## A\n- [ ] 1\n- [ ] 2\n- [ ] 3\n");
        // Slot 3 = after the last item, counted before taking item 0 out.
        assert_eq!(doc.drop_item(&ItemRef::top(0, 0), top_level(0, 3)), Some(ItemRef::top(0, 2)));
        assert_eq!(doc.render(), "## A\n- [ ] 2\n- [ ] 3\n- [ ] 1\n");
        doc.drop_item(&ItemRef::top(0, 2), top_level(0, 0));
        assert_eq!(doc.render(), "## A\n- [ ] 1\n- [ ] 2\n- [ ] 3\n");
    }

    #[test]
    fn drop_between_lists_ticks_and_unticks() {
        let mut doc = Doc::parse("## A\n- [ ] 1\n\n## Done\n- [x] 2\n");
        doc.drop_item(&ItemRef::top(0, 0), top_level(1, 1));
        assert_eq!(doc.render(), "## A\n\n## Done\n- [x] 2\n- [x] 1 <!-- from: A -->\n");
        doc.drop_item(&ItemRef::top(1, 0), top_level(0, 0));
        assert_eq!(doc.render(), "## A\n- [ ] 2\n\n## Done\n- [x] 1 <!-- from: A -->\n");
    }

    #[test]
    fn edit_rewrites_only_that_line() {
        let mut doc = Doc::parse("## A\n*   [ ] keep me\n- [ ] old\n");
        doc.edit(&ItemRef::top(0, 1), "new");
        assert_eq!(doc.render(), "## A\n*   [ ] keep me\n- [ ] new\n");
    }

    #[test]
    fn done_goes_last_in_display_order() {
        let doc = Doc::parse("## Done\n## A\n## B\n");
        assert_eq!(doc.display_order(), vec![1, 2, 0]);
    }

    #[test]
    fn sub_items_tick_in_place() {
        let mut doc = Doc::parse("## A\n- [ ] parent\n  - [ ] child\n");
        assert_eq!(doc.toggle(&sub(0, &[0, 0])), Some(sub(0, &[0, 0])));
        assert_eq!(doc.render(), "## A\n- [ ] parent\n  - [x] child\n");
    }

    #[test]
    fn all_items_walks_sub_items_in_order() {
        let doc = Doc::parse("## Done\n- [x] d\n## A\n- [ ] a\n  - [ ] a1\n    - [ ] a11\n- [ ] b\n");
        assert_eq!(doc.all_items(), vec![sub(1, &[0]), sub(1, &[0, 0]), sub(1, &[0, 0, 0]), sub(1, &[1]), sub(0, &[0])]);
    }

    #[test]
    fn dragging_a_sub_item_to_top_level_reindents_it_and_its_children() {
        let mut doc = Doc::parse("## A\n- [ ] p\n  - [ ] c\n    - [ ] gc\n    note\n- [ ] q\n");
        let at = doc.drop_item(&sub(0, &[0, 0]), top_level(0, 1)).unwrap();
        assert_eq!(at, sub(0, &[1]));
        assert_eq!(doc.render(), "## A\n- [ ] p\n- [ ] c\n  - [ ] gc\n  note\n- [ ] q\n");
    }

    #[test]
    fn dropping_into_another_parent_adjusts_for_the_removed_item() {
        // Moving top-level item 0 under item 2: once 0 is out, item 2 is at index 1.
        let mut doc = Doc::parse("## A\n- [ ] x\n- [ ] y\n- [ ] z\n  - [ ] z1\n");
        let at = doc.drop_item(&sub(0, &[0]), Place { list: 0, parent: vec![2], slot: 1 }).unwrap();
        assert_eq!(at, sub(0, &[1, 1]));
        assert_eq!(doc.render(), "## A\n- [ ] y\n- [ ] z\n  - [ ] z1\n  - [ ] x\n");
    }

    #[test]
    fn an_item_cannot_go_inside_itself() {
        let mut doc = Doc::parse("## A\n- [ ] p\n  - [ ] c\n");
        assert_eq!(doc.drop_item(&sub(0, &[0]), Place { list: 0, parent: vec![0], slot: 0 }), None);
        assert_eq!(doc.render(), "## A\n- [ ] p\n  - [ ] c\n");
    }

    #[test]
    fn sub_items_dragged_into_done_are_ticked_at_top_level_only() {
        let mut doc = Doc::parse("## A\n- [ ] p\n  - [ ] c\n\n## Done\n- [x] d\n  - [ ] d1\n");
        doc.drop_item(&sub(0, &[0, 0]), top_level(1, 0));
        assert_eq!(doc.render(), "## A\n- [ ] p\n\n## Done\n- [x] c <!-- from: A -->\n- [x] d\n  - [ ] d1\n");
        // A sub-item leaving Done keeps its state; only top-level items are unticked.
        doc.drop_item(&sub(1, &[1, 0]), top_level(0, 1));
        assert!(doc.render().starts_with("## A\n- [ ] p\n- [ ] d1\n"), "{}", doc.render());
    }

    #[test]
    fn indent_and_outdent() {
        let mut doc = Doc::parse("## A\n- [ ] a\n  - [ ] a1\n- [ ] b\n");
        assert_eq!(doc.indent(&sub(0, &[0])), None, "nothing above the first item");
        let b = doc.indent(&sub(0, &[1])).unwrap();
        assert_eq!(b, sub(0, &[0, 1]));
        assert_eq!(doc.render(), "## A\n- [ ] a\n  - [ ] a1\n  - [ ] b\n");
        let b = doc.indent(&b).unwrap();
        assert_eq!(b, sub(0, &[0, 0, 0]));
        assert_eq!(doc.render(), "## A\n- [ ] a\n  - [ ] a1\n    - [ ] b\n");
        let b = doc.outdent(&b).unwrap();
        let b = doc.outdent(&b).unwrap();
        assert_eq!(b, sub(0, &[1]));
        assert_eq!(doc.render(), "## A\n- [ ] a\n  - [ ] a1\n- [ ] b\n");
        assert_eq!(doc.outdent(&b), None, "already top level");
    }

    #[test]
    fn insert_takes_the_indent_of_its_siblings() {
        let mut doc = Doc::parse("## A\n- [ ] p\n    - [ ] c\n- [ ] q\n");
        assert_eq!(doc.insert(&Place { list: 0, parent: vec![0], slot: 1 }, "c2"), Some(sub(0, &[0, 1])));
        assert_eq!(doc.insert(&top_level(0, 1), "p2"), Some(sub(0, &[1])));
        assert_eq!(doc.render(), "## A\n- [ ] p\n    - [ ] c\n    - [ ] c2\n- [ ] p2\n- [ ] q\n");
    }

    #[test]
    fn remove_only_items_without_sub_items() {
        let mut doc = Doc::parse("## A\n- [ ] p\n  - [ ] c\n  note\n- [ ] q\n");
        assert!(!doc.remove(&sub(0, &[0])), "p has a sub-item");
        assert!(doc.remove(&sub(0, &[0, 0])));
        assert!(!doc.remove(&sub(0, &[0])), "p still has a note under it");
        assert!(doc.remove(&sub(0, &[1])));
        assert_eq!(doc.render(), "## A\n- [ ] p\n  note\n");
    }

    #[test]
    fn loose_lists_keep_sub_items_under_their_parent() {
        let text = "## A\n- [ ] p\n  - [ ] c1\n\n  - [ ] c2\n\n- [ ] q\n\n## B\n";
        let doc = Doc::parse(text);
        assert_eq!(doc.render(), text);
        let p = doc.item(&sub(0, &[0])).unwrap();
        assert_eq!(items_in(&p.children).count(), 2, "c2 is still p's sub-item");
        assert_eq!(doc.lists[0].item_entries().count(), 2, "p and q at the top level");
    }

    #[test]
    fn windows_line_endings_survive_an_edit() {
        let mut doc = Doc::parse("## A\r\n- [ ] a\r\n- [ ] b\r\n");
        doc.edit(&sub(0, &[1]), "bee");
        assert_eq!(doc.render(), "## A\r\n- [ ] a\r\n- [ ] bee\r\n");
    }

    /// Trimmed from a real hand-written TODOS.md: `#` title, prose, `###` lists, plain
    /// bullets, sub-bullets, and todos whose text continues on indented lines.
    const HANDWRITTEN: &str = "\
# TODOS

Possibili task emersi dalle conversazioni.

### General

- valutare agenzia
  - processa note vocali

- todo plugin per herdr
### Soldi e progetti

- **Provare a spiegare uno strumento** — *\"un altra cosa da provare\"* (2026-09-21). Il test
  è se piace anche la metà dello spiegare. Vedi [[provare-strumenti]].

- iscriversi anche a micro1
";

    #[test]
    fn reads_a_handwritten_file() {
        let doc = Doc::parse(HANDWRITTEN);
        assert_eq!(doc.render(), HANDWRITTEN, "unchanged on the way back");
        assert_eq!(doc.level, 3);
        assert_eq!(doc.lists.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["General", "Soldi e progetti"]);
        assert_eq!(doc.preamble[0], "# TODOS");
        let general = &doc.lists[0];
        assert_eq!(general.item_entries().count(), 2);
        let first = general.item_entries().next().unwrap();
        assert_eq!(doc.item(&sub(0, &[first, 0])).unwrap().text, "processa note vocali");
        let long = doc.item(&sub(1, &[doc.lists[1].start_slot()])).unwrap();
        assert!(long.full_text().ends_with("(2026-09-21). Il test è se piace anche la metà dello spiegare. Vedi [[provare-strumenti]]."));
        assert!(long.children.is_empty());
    }

    #[test]
    fn ticking_a_plain_bullet_gives_it_a_box_and_keeps_its_continuation() {
        let mut doc = Doc::parse(HANDWRITTEN);
        doc.toggle(&sub(1, &[doc.lists[1].start_slot()]));
        let out = doc.render();
        assert!(out.ends_with("### Done\n- [x] **Provare a spiegare uno strumento** — *\"un altra cosa da provare\"* (2026-09-21). Il test <!-- from: Soldi e progetti -->\n  è se piace anche la metà dello spiegare. Vedi [[provare-strumenti]].\n"), "{out}");
    }

    #[test]
    fn editing_a_continued_todo_makes_it_one_line() {
        let mut doc = Doc::parse("## A\n- first part\n  second part\n- next\n");
        assert_eq!(doc.item(&sub(0, &[0])).unwrap().full_text(), "first part second part");
        doc.edit(&sub(0, &[0]), "first part second part!");
        assert_eq!(doc.render(), "## A\n- first part second part!\n- next\n");
    }

    #[test]
    fn new_todos_follow_their_siblings_style() {
        let mut doc = Doc::parse("## A\n* plain\n");
        doc.insert(&top_level(0, 1), "also plain");
        assert_eq!(doc.render(), "## A\n* plain\n* also plain\n");
    }

    #[test]
    fn bullets_without_a_heading_are_an_unnamed_general_list() {
        let mut doc = Doc::parse("# My todos\n\n- a\n- b\n");
        assert_eq!(doc.lists.len(), 1);
        assert_eq!(doc.lists[0].name, GENERAL);
        doc.add("c");
        assert_eq!(doc.render(), "# My todos\n\n- a\n- b\n- c\n");
        doc.rename_list(0, "Home");
        assert_eq!(doc.render(), "# My todos\n\n## Home\n- a\n- b\n- c\n");
    }

    #[test]
    fn several_top_level_headings_are_lists() {
        let doc = Doc::parse("# Work\n- a\n# Home\n- b\n");
        assert_eq!(doc.lists.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Work", "Home"]);
    }

    #[test]
    fn adding_renaming_and_removing_lists() {
        let mut doc = Doc::parse("### A\n- a\n\n### Done\n- [x] d\n");
        let b = doc.add_list("B");
        assert_eq!(b, 1, "before Done");
        assert_eq!(doc.render(), "### A\n- a\n\n### B\n\n### Done\n- [x] d\n", "at the file's own level");
        doc.insert(&top_level(1, doc.lists[1].end_slot()), "b1");
        assert_eq!(doc.render(), "### A\n- a\n\n### B\n- b1\n\n### Done\n- [x] d\n");
        doc.rename_list(1, "Bee");
        assert!(doc.render().contains("### Bee\n- b1\n"));
        assert!(!doc.remove_list(1), "has a todo");
        doc.remove(&sub(1, &[0]));
        assert!(doc.remove_list(1));
        assert_eq!(doc.render(), "### A\n- a\n\n### Done\n- [x] d\n");
    }

    #[test]
    fn list_colours_round_trip_and_stay_out_of_the_name() {
        let text = "## Work <!-- color: blue -->\n- [ ] a\n";
        let mut doc = Doc::parse(text);
        assert_eq!((doc.lists[0].name.as_str(), doc.lists[0].color.as_deref()), ("Work", Some("blue")));
        assert_eq!(doc.render(), text);
        doc.rename_list(0, "Job");
        assert_eq!(doc.render(), "## Job <!-- color: blue -->\n- [ ] a\n");
        doc.set_list_color(0, None);
        assert_eq!(doc.render(), "## Job\n- [ ] a\n");
        // The unnamed list gets a heading to hold its colour.
        let mut doc = Doc::parse("- [ ] a\n");
        doc.set_list_color(0, Some("red"));
        assert_eq!(doc.render(), "## General <!-- color: red -->\n- [ ] a\n");
    }

    #[test]
    fn deleting_a_list_takes_its_todos_and_keeps_the_spacing() {
        let mut doc = Doc::parse("## A\n- [ ] a\n\n## B\n- [ ] b\n  - [ ] b1\n\n## Done\n- [x] d\n");
        assert_eq!(doc.delete_list(1).map(|l| l.name), Some("B".to_string()));
        assert_eq!(doc.render(), "## A\n- [ ] a\n\n## Done\n- [x] d\n");
        doc.delete_list(1);
        assert_eq!(doc.render(), "## A\n- [ ] a\n");
        assert!(doc.delete_list(5).is_none());
    }

    #[test]
    fn moving_lists_keeps_blank_lines_between_them_and_done_last() {
        let mut doc = Doc::parse("## A\n- a\n\n## B\n- b\n\n## Done\n- [x] d\n");
        assert_eq!(doc.move_list(1, Some(0)), Some(0));
        assert_eq!(doc.render(), "## B\n- b\n\n## A\n- a\n\n## Done\n- [x] d\n");
        assert_eq!(doc.move_list(0, None), Some(1), "after the last list, still before Done");
        assert_eq!(doc.render(), "## A\n- a\n\n## B\n- b\n\n## Done\n- [x] d\n");
        assert_eq!(doc.move_list(2, Some(0)), None, "Done stays put");
        assert_eq!(doc.move_list(0, Some(1)), None, "already right before B");

        // No Done, no blank line at the end: the last list moving up gets one, the new last loses it.
        let mut doc = Doc::parse("## A\n- a\n\n## B\n- b\n");
        doc.move_list(1, Some(0));
        assert_eq!(doc.render(), "## B\n- b\n\n## A\n- a\n");
        doc.move_list(0, None);
        assert_eq!(doc.render(), "## A\n- a\n\n## B\n- b\n");
    }

    #[test]
    fn the_unnamed_list_gets_a_heading_when_another_moves_above_it() {
        let mut doc = Doc::parse("- a\n\n## Work\n- w\n");
        doc.move_list(1, Some(0));
        assert_eq!(doc.render(), "## Work\n- w\n\n## General\n- a\n");
    }

    #[test]
    fn delete_takes_sub_items_and_says_where_the_cursor_goes() {
        let mut doc = Doc::parse("## A\n- [ ] a\n- [ ] b\n  - [ ] b1\n  note\n- [ ] c\n");
        let (gone, next) = doc.delete(&sub(0, &[1])).unwrap();
        assert_eq!(gone.text, "b");
        assert_eq!(doc.render(), "## A\n- [ ] a\n- [ ] c\n");
        assert_eq!(next, Some(sub(0, &[1])), "c slid into b's place");
        let (_, next) = doc.delete(&sub(0, &[1])).unwrap();
        assert_eq!(next, Some(sub(0, &[0])), "nothing after: the one before");
        let mut doc = Doc::parse("## A\n- [ ] p\n  - [ ] only\n");
        assert_eq!(doc.delete(&sub(0, &[0, 0])).unwrap().1, Some(sub(0, &[0])), "last sub-item: its parent");
    }
}
