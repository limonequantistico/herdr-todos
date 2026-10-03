# herdr-todos

_Name taken from the folder. Alternatives: **herdr-list** (short, says what it is), **tick** (one syllable, terminal-feel), **sidetodo** (hints at the side panel)._

## Vision
A plugin for the terminal software [herdr](https://herdr.dev/) that puts simple todo lists in a side panel, next to the work. It's for developers who live in the terminal and want to capture and reorder tasks without leaving it or opening another app. The point is that it's immediate, simple, and reliable: drag to reorder, click to tick off, and type to add.

## One-liner
Basecamp-style todo lists in a herdr side panel, stored in a plain `TODOS.md` file per folder.

## Target Users
1. **Terminal-first developer using herdr** — comfortable with TUIs and markdown. Frustrated that todo apps are heavy, need a context switch, or keep data somewhere opaque. Expects instant capture, keyboard-driven use, and files they can read and commit.
2. **Dev juggling several projects** — Wants a different list per folder without any setup. Expects the todos to follow the opened folder automatically.

## Core Features
1. **Side panel with lists** — opens inside herdr and shows the available lists, each with a title and ordered items.
2. **`TODOS.md` as the source of truth** — if present in the current folder, it defines all lists and items in plain markdown. Different folders get different todo files. Format: each `## Heading` is a list, `- [ ]` / `- [x]` lines are items, indented items are sub-items; anything else in the file is kept untouched. With no `TODOS.md`, the panel opens empty and creates the file on the first add.
3. **Draggable, sortable items** — reorder items, and move them between lists, by mouse drag and by keyboard; while dragging, a ghost of the item follows the pointer and a gap previews where it will land. The look is modeled on Basecamp's todos (reference screenshots in `.docs/assets/imgs/references/`: a Claude Code onboarding checklist card, and a Basecamp list view with colored-dot list titles, checkbox items, "Add a to-do" links, a "New list" button and a filter).
4. **Click to tick off** — a single click (or a key) checks an item off and moves it to a **Done** list, always kept at the bottom. Unticking an item in Done sends it back to General. Both are written to `TODOS.md` immediately.
5. **Write like in a notes app** — click an item's text and start typing; Return saves and opens a new todo on the line below, Return on an empty line stops. Each list ends with "+ Add a to-do" to start writing there. Clearing an item's text deletes it (never one with sub-items or notes under it). Dragging across text still selects and copies it. No shortcuts needed.
6. **Quick-add** — the box at the top takes todos that don't belong anywhere yet: they go into a General list, from which they can be dragged into the right list. [CHANGED: automatic filing with Laya is dropped for now — it needs a Python install, a large model and a background server, too heavy for a simple todo app. See Later.]
7. **Polished TUI feel** — built with [ratatui](https://ratatui.rs/) in a small right-hand split pane. A spike (`spikes/mouse-drag/`) confirmed drag with a ghost and drop preview, text selection, and click-to-tick work inside herdr.
8. **Sub-items** — todos nested under a todo, as indented checkboxes. `Tab` nests an item under the one above, `Shift+Tab` moves it back out. Dragging a parent moves its sub-items with it; dropping on a sub-item row nests the dragged item there. Sub-items tick in place; only top-level items move to Done. A todo's sub-items can be collapsed by clicking the `⏷` after it. Text editing follows macOS habits as far as the terminal passes them on (Option+←/→ by word, Cmd+←/→ to line ends, Cmd+Backspace); Ctrl+Z / Ctrl+Y undo and redo, since Ghostty keeps Cmd+Z for itself.

## Later
- **Creating, renaming and deleting lists from the panel** (the Basecamp "New list" button). For now, lists are `##` headings you write in `TODOS.md`; the panel creates only General and Done, the first time it needs them.
- **Reorder button for General** — files the not-yet-sorted items in General into the matching lists in one go, using [Laya](https://github.com/NandhaKishorM/laya) (or Jev, once off its waitlist). Only worth it if it can stay optional and light.
- **herdr-boy control center** — moved to its own project, `~/CodingProjects/herdr-boy`. A green-phosphor control center with tabs (STATS, clock, TODOS…) that would include this plugin's lists. Start it only once the todos feature here is complete.
- **A version for the [Pi](https://pi.dev) coding agent** — a separate TypeScript extension, since Pi extensions draw their own components inside Pi. It would share the same `TODOS.md`, so lists stay in sync. Pi's docs describe keyboard input for extension components, so drag and drop probably wouldn't carry over.

## Non-Goals
- Not a full task manager: no due dates, assignees, sync, or accounts.
- No database or hidden storage; the markdown file is the only data.
- Reliability and simplicity win over features and effects.

## Existing Alternatives
Searched 2026-10-02. None is built on herdr, and none combines drag-and-drop between lists with click-to-tick on a plain `TODOS.md`. That combination, inside the herdr workspace, is what this project adds.
- **[TodoMD](https://github.com/walm/todomd)** — closest. Kanban TUI and CLI over a plain `TODO.md`, keyboard reorder and mouse clicks, found from the working directory upward. Falls short: board/card model rather than Basecamp-style lists, no mouse drag mentioned, no auto-filing, no herdr or multiplexer integration.
- **[Sidecar](https://github.com/than/sidecar)** — Markdown todo list in a narrow side pane next to a Claude Code session, with click or keyboard ticking. Falls short: the list belongs to the AI agent; the user can tick items and answer questions but can't move items between sections, and there's no drag or automatic filing.

Also seen, not direct competitors: [pi-todo-herdr](https://cdn.jsdelivr.net/gh/leset0ng/pi-todo-herdr@main/README.md) (task tree an AI agent manages in Pi; shows the current task in herdr's sidebar via metadata tokens) and [d-note](https://docs.rs/crate/d-note/0.1.3) (Markdown sticky note in a floating tmux/zellij pane).
