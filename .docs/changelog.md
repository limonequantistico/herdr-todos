# Changelog

## 2026-10-02

### 23:10

  - ran `/init` — scaffolded .docs/ (idea, backlog, changelog, changelog-spec, prototype-prompt, asset folders), .gitignore, CLAUDE.md

### 23:23

  - ran `/seed` — created .docs/seed.md from idea.md notes

### 23:28

  - ran `/seed` — integrated answers: Laya chosen, mouse+keyboard reorder, searched for alternatives

### 23:37

  - reviewed /init and /seed output; corrected alternatives (added Sidecar) and screenshot description in seed.md

### 23:43

  - `/seed` — removed MarkdownDO, verified Sidecar, added click-to-tick and auto-filing fallback

### 23:46

  - ran `/stack` — Rust + ratatui herdr plugin, TODOS.md line parser, optional Laya server

### 23:54

  - dropped Laya auto-filing from MVP; reorder-General button moved to Later in seed
  - built mouse-drag spike in spikes/mouse-drag and linked it into herdr

## 2026-10-03

### 00:05

  - spike: drag-to-reorder and text selection confirmed working in a herdr pane
  - added drag ghost + drop preview to spike; added edit-in-place to seed

### 00:07

  - seed: ticked items move to a bottom Done list; list management moved to Later

### 00:13

  - spike: checkbox fixed to ▢ → ✓ in accent color (one-column glyphs, no misalignment)

### 00:16

  - spike: back to markdown-style [ ] / [x] checkboxes

### 00:20

  - spike: Pip-Boy-style green phosphor look, toggled with t

### 00:24

  - spike: phosphor colours sampled from Pip-Boy screenshot; boxed footer like the HP/LEVEL/AP strip

### 00:25

  - spike: phosphor background darkened to near-black

### 00:26

  - spike: fixed unreadable text selection on the cursor row (bright text on panel fill)

### 00:30

  - seed: parked Pip-Boy control center idea under Later; feature 7 updated with spike results

### 00:41

  - drafted .docs/design-system.md from the spike (symbols provisional)
  - seed: herdr-boy moved to its own project; Pi coding agent version added to Later

### 00:59

  - built the real panel: TODOS.md model (15 tests), lists, General/Done, quick-add, edit, drag between lists, live reload
  - added the toggle action: opens a narrow right-hand panel in the focused folder, closes it if open

### 01:09

  - sub-items: tick in place, drag at any level with an indented ghost, Tab / Shift+Tab to nest and un-nest, edit (22 tests)

### 01:17

  - writing like a notes app: click text to write, Return opens the next line, + Add a to-do per list, clearing deletes, Backspace on empty goes up

### 01:30

  - long todos wrap (also while typing); sub-items collapse with ▾ / ▸ n; phosphor background now black; shift+tab unnest in the hint line

### 01:39

  - mac text keys (Option/Cmd+arrows, Option/Cmd+Backspace as Ghostty sends them), Ctrl+Z / Ctrl+Y undo and redo, bigger accent collapse marker, grip click no longer un-nests

### 01:45

  - writing: no underline, the grip lights up instead (also on new lines)

### 01:49

  - indent guides: a dim │ per nesting level under each parent, as in VS Code / IntelliJ

### 01:52

  - the current block's indent guide lights up (accent), like IntelliJ
  - bound the toggle to prefix+t in herdr config (prefix+shift+t is rename tab)

### 02:00

  - press and drag anywhere on a todo moves it; a click (or a shaky one) starts writing; selecting text now happens inside the line being written
  - indent guide highlight: the line below the selected todo, heavier ┃

### 02:01

  - personal dev-layout plugin (~/.config/herdr/local-plugins/dev-layout) on prefix+shift+o: dev tab (terminal | todos over reviewr) + memex tab

### 02:08

  - clicking the grip now starts writing (it did nothing; found by logging real clicks); dev layout is now an even half / quarter / quarter split

### 02:15

  - guide lines start under the grip of a wrapped todo instead of after its last row; a grip click only selects

### 02:17

  - collapse marker moved to the right edge of the todo's first row, clear of the text

### 02:22

  - indent guides close with a corner (╰, ┗ when lit) on the last row of their block

### 02:30

  - ran `/herdr-review` (Claude Opus): 8 findings, all accepted and fixed: redo / Tab / reload while writing could write over another todo; untouched lines no longer rewritten; loose lists keep sub-items; CRLF kept; symlinked TODOS.md followed; Ctrl+C saves (36 tests)

### 02:34

  - ran `/push` — first commit, pushed to github.com/limonequantistico/herdr-todos

---  `v0.1.0 released`

### 02:46

  - reads hand-written TODOS.md files: list headings at any level, plain bullets, continued lines, an unnamed list before any heading (checked on cogi's real file)
  - lists from the panel: + New list, click a title to rename, clear an empty one to remove it (45 tests)

### 02:51

  - wheel scrolling no longer snaps back to the cursor; a focused wrapped todo stays fully on screen
  - lists collapse from a marker on their title (magenta / amber), Done included (47 tests)

### 03:00

  - delete key removes the selected todo with its sub-items; ctrl+z restores it (49 tests)

### 03:05

  - ran `/herdr-review` (Claude Opus): fixed clicks landing on the wrong row after a list rename/new list, delete/tick acting on a todo hidden in a collapsed list, empty-state hint overlapping "+ New list"; continuation-line flattening on edit left open

---  `v0.2.0 released`

### 09:11

  - clicking empty space or pressing Esc clears the selected todo; arrows pick it back up (50 tests)

### 09:22

  - drag a list title to reorder lists (only titles show while dragging, Done stays last); a click still renames (53 tests)

### 09:31

  - r reloads TODOS.md by hand; the status line says so when the file watcher could not start (54 tests)

### 09:40

  - an open panel restarts itself in place when its binary is rebuilt or updated (waits until nothing is being written or dragged); no more closing and reopening panes

### 09:58

  - theme and collapsed lists/todos are remembered (per TODOS.md, in herdr's plugin state folder); a restart on a new build also keeps undo history, selection and scroll; the binary is checked once a second (57 tests)

### 10:25

  - ran `/herdr-review` (Claude Opus): a list can now be dragged to the end when there is no Done list; a malformed state.json no longer crashes the panel; one panel no longer resets the theme another panel chose; a key press during a drag cancels it (59 tests)

---  `v0.3.0 released`

### 10:31

  - "+ New list" moved to the right end of the quick-add row, so it stays in view however far the lists scroll; the new list's name is typed where the list will go (60 tests)
  - quick add handles long text: it scrolls sideways to keep the cursor in view, shows `…` where it's cut, never runs into "+ New list", and a click puts the cursor where it lands (61 tests)
  - keys: `n` starts a new todo right below the selected one (or at the end of the first list), `l` starts naming a new list (62 tests)
  - ran `/herdr-review` (Claude Opus): a click that drops an unnamed new list no longer lands on the row below (it could tick the wrong todo); panels too narrow for the top-row button get the in-list "+ New list" row back; wide characters in quick add left for later (63 tests)
  - emoji and CJK text line up: wrapping, the quick-add box, the text cursor, clicks and selection count terminal columns, not characters (adds unicode-width, already in the build via ratatui) (64 tests)

---  `v0.4.0 released`

## 2026-10-03

### 10:58

  - added an MIT license (LICENSE, `license = "MIT"` in Cargo.toml, README note), matching the herdr plugin ecosystem

## 2026-10-04

### 01:56

  - README: install, toggle binding, TODOS.md format, mouse and key cheat sheet
  - README: markdown example replaced by a screenshot slot; keys section notes the bottom line shows the keys for the current mode
  - unticking a todo in Done sends it back to the list it came from (remembered as a `<!-- from: List -->` note in TODOS.md, kept up to date when the list is renamed), falling back to General (67 tests)
  - list menu: clicking a list's dot opens colour swatches and delete; colours are saved as `<!-- color: red -->` on the heading, deleting takes the list with its todos (ctrl+z brings it back) (70 tests)
  - lists can be dragged by their dot too; a click on the dot still opens the list menu (70 tests)
  - README: demo GIF at the top (two screen recordings joined, `.docs/assets/imgs/demo.gif`, 2.6 MB)
  - README: removed the empty screenshot slot

---  `v0.5.0 released`
  - marketplace prep: manifest and Cargo versions set to 0.5.0 (they had stayed at 0.1.0 through every cut), removed the mouse-drag spike's herdr-plugin.toml so the marketplace doesn't list it (spike code kept), set the GitHub repo description
  - plugin display name changed to "todos" (id and repo stay herdr-todos)

## 2026-10-10

### 19:47

  - the panel follows the folder: it shows the TODOS.md of the terminal pane beside it in its tab, re-checked once a minute off the drawing loop, and switches only when nothing is being written or dragged; a restart on a new build keeps the folder it moved to (73 tests)

---  `v0.6.0 released`
