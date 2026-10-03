# Design System

Drafted 2026-10-03 from the mouse-drag spike (`spikes/mouse-drag/`). Symbols are provisional — easy to change, since they live in one place in code.

## Principles

- **Immediate.** Every gesture shows its result on the next frame: a tick, a drop, a selection.
- **Reads like the file.** The panel mirrors `TODOS.md`; markdown-style checkboxes keep the two recognisably the same thing.
- **Themes are value sets, not code paths.** Widgets use colour *roles*, never raw colours. A theme only fills in the roles. Never hardcode a colour in a widget.
- **One column per symbol.** Only use characters Unicode marks as narrow (East Asian Width N or Na). Ambiguous-width ones (`▣`, Nerd Font icons) render two columns wide in some terminals and push the row out of line — this broke alignment in the spike. The one exception is line drawing (`│`, `┄`): formally ambiguous too, but drawn one column wide by every terminal, since TUIs depend on it for borders.

## Colour roles

| Role | Used for | Plain | Green phosphor |
|---|---|---|---|
| `fg` | Item text, titles, lines | terminal default | `rgb(105,255,125)` |
| `dim` | Grips, ticked items, drop slot, hints | `DarkGray` | `rgb(62,112,60)` |
| `bg` | Panel background | terminal default | `rgb(0,0,0)` |
| `on_bar` | Text on a solid `fg` bar | `Black` | `rgb(0,0,0)` |
| `panel` | Box fills, text selection | `Blue` | `rgb(18,48,22)` |
| `accent` | Cursor marker when there's no bar | `Cyan` | — (the bar marks the cursor) |
| `status` | Transient messages ("copied: …") | `Yellow` | `dim` |
| `list_marker` | Collapse marker on list titles, apart from the todos' (`accent`) | `Magenta` | `rgb(255,182,66)` (amber) |

**Plain** uses the terminal's own colours, so it follows whatever theme the user already has. It's the default. The chosen theme is remembered (in herdr's plugin state folder) for every panel.
**Green phosphor** is an optional theme. Its greens were sampled from a Fallout 4 Pip-Boy screenshot; the background is pure black, like a CRT terminal. Never ship it under the Fallout or Pip-Boy names.

## Symbols (provisional)

| Symbol | Meaning |
|---|---|
| `⠿` | Grip — drag from here to move an item |
| `[ ]` / `[x]` | Unticked / ticked, as in markdown |
| `┄┄┄` | Drop slot — where a dragged item will land |
| `●` | List title marker (Basecamp's coloured dot) |
| `╰` / `┗` | A guide's last row: the line turns into a corner where its block ends (`┗` for the lit one), so a block's end is visible |
| `│` | Indent guide: one per level, `dim`, under each ancestor's grip, on every sub-item row (wrapped rows and the drop slot included). A todo whose sub-items show starts its line right under its grip, through its own wrapped rows. The current block's guide is a heavier `┃` in `accent`: the line right below the todo under the cursor (or being written) when its sub-items show, else the line of the block it sits in |
| `⏷` / `⏵ 3` | At the right edge of the first row of a todo with sub-items, bold `accent`: shown / collapsed (with how many are hidden). Click it (or the column left of it) to toggle. Text wraps two columns short of it, so clicking a todo's end always edits. List titles with todos carry the same marker in `list_marker`; a collapsed list shows only its title and `⏵ n` (all todos inside, sub-items included) |

Rejected so far: `▢`/`▣` and Nerd Font boxes (alignment), `☐`/`☑` (inconsistent size across fonts).

## Layout

The panel is a small right-hand herdr split, about a third of the tab when it opens at the tab's right edge. The last row is a status line: the latest message, or key hints. Columns per item row:

| Columns | Content | Mouse |
|---|---|---|
| 0–1 | grip + space | press and drag → move item; click → select it (no writing, so a barely-moved drag never opens an edit). With a todo selected, delete (Backspace or Delete) removes it and its sub-items; the status line offers ctrl+z |
| 2–5 | `[ ] ` | click → tick / untick |
| 6+ | text | click → write there; press and drag → move item (once the pointer leaves the row or travels 2 columns). While writing: drag across the line → select and copy |

Sub-items shift the whole row (grip, box, text) two columns right per level, and work like any other item. The first of each two indent columns holds a `dim` `│` guide.

Rows, top to bottom: quick-add box (`+ Quick add, goes to General`, `dim` when empty), a spacer, lists in `TODOS.md` order with General first, **Done** always last. Every list except Done ends with a `dim` underlined `+ Add a to-do` row, its text aligned with item text (as in Basecamp). After the last list before Done, a `dim` `+ New list` row (underlined label, aligned with list titles). One blank row between lists. Clicking a list title (except Done) edits its name in place, like a todo; dragging it moves the list. While a list is dragged only list titles show, a `┄` slot marks where it lands (above the title under the pointer; on Done, at the end), and a ghost `● name` follows the pointer. Done never moves.

## States

- **Cursor** — plain: the grip turns `accent`. Phosphor: the whole row becomes a solid `fg` bar with `on_bar` text, running the full panel width. The panel opens on the first todo. Clicking empty space (blank rows, below the lists, the quick-add box) or Esc clears it, so nothing is selected; ↓/↑ then pick up from the top/bottom.
- **Ticked** — text crossed out and `dim`. The item moves to Done.
- **Dragging** — the item leaves its place (with its sub-items) and the list closes up; a `dim` drop slot opens where it would land; a ghost (solid bar: `accent` in plain, `fg` in phosphor) follows the pointer row by row, indented to the level it would land at. Dropping on a row puts the item at that row's level, just before it.
- **Text selection** — `panel` fill with `fg` text, including on the cursor row (where `on_bar` text would vanish against the fill).
- **Writing** — the line's grip lights up (`accent`, or the bar in phosphor) and the text cursor shows where you type; no underline; the status line shows `enter next line · tab nest · esc done`. A new line being typed shows as an item row and joins the file once it has text. Esc and clicking elsewhere both save; there is no cancel.
- **Empty list** — just the title and its `+ Add a to-do` row. An empty Done list shows a `dim` italic "nothing here" row.
- **No `TODOS.md`** — the quick-add input is focused and a `dim` hint reads "No TODOS.md here yet. Add a to-do to start one." A file with no todos or lists yet reads "No todos in TODOS.md yet. Add one above, or start a list."
- **Not watching** — if the file watcher can't start (network drives, Linux out of watches), the status line reads `not watching TODOS.md for outside edits · r to reload` in `status`. `r` reloads by hand whenever nothing is being written or dragged; it's left out of the normal key hints, since edits show up on their own.
- **Markdown in todo text** (`**bold**`, `[[links]]`) is shown as written, not rendered.
- **Long text** — wraps onto extra rows, breaking after a space where possible; later rows are aligned under the text (no grip or box). Wrapping follows the text live while typing.
- **Collapsed** — a todo's sub-items are hidden and its marker reads `⏵ n`. Collapsed lists and todos are remembered per `TODOS.md`, across sessions. Nesting a line under a collapsed todo opens it.

## Typography

The terminal decides the font. List titles are bold. Phosphor uses uppercase for chrome (tab bar, footer labels) to suggest the Fallout lettering; item text is never changed.

## Motion

Effects (via tachyonfx) are decoration only: the panel must look and work the same with them off. None are designed yet.

## Phosphor-only chrome

The spike also tried a tab-bar header (`┌ TITLE ┐` rising from a rule) and a boxed footer (done count, progress meter, percentage). These belong to the [herdr-boy](../../herdr-boy) control-center idea, not to the todos panel.
