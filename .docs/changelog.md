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
