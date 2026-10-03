# Tech Stack

A herdr plugin: one Rust binary that draws the todo panel in a small herdr side pane. No backend, no database, no hosting — the data is `TODOS.md` in the user's folder.

Versions checked 2026-10-02 (crates.io, PyPI, herdr releases).

## Host: herdr plugin

- **herdr ≥ 0.9** — `min_herdr_version = "0.9.0"` in `herdr-plugin.toml` [TBD: lower it once the spike shows which APIs we actually need]. Latest stable is 0.9.3; 0.9.1 is installed locally.
- **Manifest** — `herdr-plugin.toml` with:
  - one `[[panes]]` entrypoint (`panel`) that runs the binary;
  - one `[[actions]]` entry (`toggle`) that opens or closes the panel. herdr binds keys to *actions*, not panes, so the user adds a `[[keys.command]]` with `type = "plugin_action"` pointing at `toggle`.
- **The "side panel" is a split pane.** herdr has no plugin sidebar API. The panel opens with `placement = "split"`, `--direction right` (herdr only splits right or down, so it can't open on the left), and is narrowed with `pane.resize`. It's a normal herdr pane after that: it can be moved, swapped, or closed like any other.
- **Which folder?** The `toggle` action reads the focused pane from `HERDR_PLUGIN_CONTEXT_JSON`, asks `herdr pane get` for its cwd (`foreground_cwd` when available), and opens the panel with `--cwd` set to it. The panel then looks for `TODOS.md` there.
- **Talking to herdr** — through the CLI at `HERDR_BIN_PATH`, not the raw socket, so it works the same on every OS.

## Panel UI (Rust)

- **Rust, edition 2024** (local toolchain: rustc 1.98).
- **[ratatui](https://ratatui.rs/) 0.30** — the TUI. Use its re-exported `ratatui::crossterm` rather than adding crossterm separately, so the two never drift apart.
- **crossterm 0.29** (via ratatui) — keyboard input and mouse events: click to tick, `Drag` events for drag-and-drop.
- **[tachyonfx](https://github.com/ratatui/tachyonfx) 0.25** — the "nice effects" (fade on tick-off, slide when an item is filed). Small and built for ratatui 0.30. Effects are decoration; the panel must work identically with them off.
- **notify 8.2** — watches `TODOS.md`, so edits from an editor or an AI agent show up in the panel immediately.
- **anyhow 1.0** — error handling in the binary.
- **serde_json 1.0** — reading herdr's context JSON and CLI output.

### `TODOS.md` reading and writing

A small hand-written line parser, not a markdown library. The format is deliberately narrow (`## List name` headings, `- [ ] item` / `- [x] item` lines), and the file must round-trip: any line we don't understand (notes, blank lines, other markdown) is kept exactly where it was. Markdown crates such as pulldown-cmark parse well but can't write a document back unchanged.

Writes are atomic — write to a temp file in the same folder, then rename — so a crash or a concurrent editor never leaves a half-written file. std is enough; no extra crate.

[TBD: exact format and the "no `TODOS.md` yet" behavior — decide during the first build task.]

## Later: automatic filing

Dropped from the MVP (2026-10-02). [Laya](https://github.com/NandhaKishorM/laya) needs Python ≥ 3.10, a 322–421M-parameter model and a long-running `laya-serve` process to answer quickly — too heavy for a simple todo panel. If the "reorder General" button comes back, the notes still hold: call a warm `laya-serve` over HTTP with the list names as `choice` options, keep it optional, and try [laya-mlx](https://pypi.org/project/laya-mlx/) on Apple Silicon.

## Dev tools

- **cargo fmt** and **cargo clippy** (`-D warnings`).
- **cargo test** (plain assertions so far; add **[insta](https://insta.rs/) 1.48** once rendered frames are worth snapshotting) for the places bugs would hurt most: `TODOS.md` round-trips (parse → change → write keeps everything else intact) and the list operations.
- **Local loop:** `herdr plugin link .`, then `cargo build` and reopen the pane. `link` doesn't run build commands, so you build yourself.
- **CI:** GitHub Actions — fmt, clippy, test on macOS and Linux.

## Distribution

- **GitHub repo tagged `herdr-plugin`**, installed with `herdr plugin install owner/herdr-todos`. It appears in the herdr marketplace automatically.
- **MVP build:** a `[[build]]` step runs `cargo build --release` on install. That means users need a Rust toolchain — fine for early adopters.
- **Later:** prebuilt binaries attached to GitHub releases, with a small build step that downloads the right one, so users don't need cargo.

## Feasibility: verify first

The herdr docs don't settle these. A short spike should answer them before any real building:

1. **Mouse drag inside a pane, alongside text selection.** herdr captures the mouse by default (`ui.mouse_capture = true`) and uses left-drag for text selection. Its docs say mouse events reach apps that turn on mouse reporting, but they don't explicitly cover left-drag in a normal split pane. We want both: dragging items, and still being able to select text. **If drag doesn't arrive, drag-and-drop has to be rethought** (keyboard moves and click-to-pick-up still work). This is the riskiest assumption — spike in `spikes/mouse-drag/`.
   **Confirmed by the spike (2026-10-03):** drag-to-reorder with a ghost and drop preview, text selection with copy, and click-to-tick all work in a herdr right split.
   **Background:** [reviewr](https://github.com/persiyanov/herdr-reviewr), a Rust + ratatui 0.30 herdr plugin in a right split, receives left-drag in its pane and does its own text selection, copying through `pbcopy` / `wl-copy` / `xclip` / `xsel`. Once a pane captures the mouse, herdr's own text selection no longer applies there, so the panel has to provide both gestures itself: drag from the grip (⠿) moves an item, drag across the text selects it.
2. **Panel width and placement** — whether `pane.resize` can hold a narrow right-side split, and how it behaves when the window resizes.
3. **Folder changes** — whether the panel should follow focus to another workspace's folder, or stay one panel per workspace.
