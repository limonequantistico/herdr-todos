# Changelog format (canonical)

All commands that write to the changelog must follow this spec. The file is **`.docs/changelog.md`**.

## Structure

1. **Optional title** — First line may be `# Changelog`. If the file is created from scratch, include this line.
2. **Blank line** — After the title, if present.
3. **Date sections** — Entries are grouped under day headers:

```markdown
## YYYY-MM-DD
```

4. **Time blocks** — Entries written in the same batch are grouped under a time header:

```markdown
### HH:mm
```

5. **Entries** — Plain bullet lines under the current time block, indented by two spaces.
6. **Release separators** — `/version` may insert a release separator line between groups:

```markdown
---  `v0.2.0 released`
```

## Writing rules

- Use the **actual** current local date and time when writing. Do not invent timestamps.
- New entries should be appended in chronological order within the current day section.
- If today's `## YYYY-MM-DD` section already exists, append within it instead of creating a duplicate.
- If the current `### HH:mm` block already exists inside today's section, append to that block.
- If today's section exists but the current time block does not, create a new `### HH:mm` block at the bottom of today's section.
- If the file does not exist yet, create it with `# Changelog`, a blank line, today's `## YYYY-MM-DD`, and the current `### HH:mm`.
- Do not reorder old entries or move existing release separators.
- Indent bullets under each `### HH:mm` block by two spaces.

## Entry types

### Completed work

Use a plain bullet with the user's original wording. Do **not** add `[x]`.

```markdown
  - completed item
```

### Command / activity log

Wrap command names in backticks.

```markdown
  - ran `/command-name` — short description
```

### Short command tag

Short command tags should also use backticks.

```markdown
  - `/drift` — ≤10 words
```

### Untimed notes

Avoid untimed notes when writing new entries. If an existing changelog already contains older untimed material, preserve it.

## Version cuts (`/version`)

When `/version` processes the log, it **does not delete** prior entries. It inserts a **separator line** immediately **after** the block of entries that were rolled into that release:

```markdown
---  `v0.2.0 released`
```

Use exactly three dashes, two spaces, and a backticked label in the form `` `vX.Y.Z released` ``.

The next `/version` run treats entries **below** the latest separator as “since last release.”

## Rules

- Do not edit the user’s wording when moving backlog items into the changelog unless a command explicitly says to normalize it.
- Keep entries as single-line bullets where practical.
- Do not add `[x]` markers.
- Do not add extra categories inside a time block.
- This file is **tracked in git** so the team shares the same history.
