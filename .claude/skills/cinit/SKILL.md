---
name: cinit
description: Initialize project context for the first time. Reads the repo, asks the user targeted questions, and creates .claude/context/ABOUT.md and .claude/context/PLAN.md. If these files already exist, updates them instead. Run this once per project to give Claude background on what it is and how to approach it.
---

## What This Skill Does

Creates two files in `.claude/context/`:
- **ABOUT.md** — concise, accurate description of the project's purpose, stack, structure, and conventions
- **PLAN.md** — step-by-step approach guide covering current phase, ordered tasks, what's done, and what's next

If either file already exists, updates it rather than starting from scratch.

---

## Step 1: Check for Existing Files

Check whether `.claude/context/ABOUT.md` and `.claude/context/PLAN.md` exist.

Report what was found:
> "Found: {list of existing files, or 'neither file'}. {Creating|Updating} context."

Treat existing files as updates — preserve accurate sections, only rewrite what has drifted or is missing.

---

## Step 2: Explore the Repo

Read the project to build a factual foundation before asking anything.

**Identity & build:**
- `package.json` / `Cargo.toml` / `go.mod` / `pyproject.toml` — framework, dependencies, scripts
- `vite.config.*` / `next.config.*` / `webpack.config.*` — build config
- `README.md` — stated purpose or architecture
- `git remote get-url origin` — repo name and host

**Documentation:**
- Check for a `docs/`, `documentation/`, `doc/`, or `wiki/` directory at the repo root — if found, read all files inside
- Also check for any `*.md` files at the root beyond README (e.g. `CONTRIBUTING.md`, `ARCHITECTURE.md`, `DEVELOPMENT.md`)
- Extract all meaningful information: architecture decisions, setup instructions, conventions, API descriptions, data models, workflow guides

**Structure:**
- Top-level directory listing
- `src/` or equivalent — subdirectory layout
- Where does data live? Where do pages/routes live? Where do components/modules live?

**Key patterns:**
- Main entry point
- Router or routing config
- Data files or schemas (read 2–3 representative ones)
- State management approach if visible
- File naming conventions

**Project state signals** (for PLAN.md):
- Are there open TODOs or FIXMEs in the code?
- Is there a `CHANGELOG.md`, `ROADMAP.md`, or issues list?
- What looks incomplete, scaffolded, or in-progress?
- What appears production-ready vs experimental?

Do not ask the user anything yet.

---

## Step 3: Interview for Gaps

Ask only what the repo cannot answer. One question at a time, wait for each answer before asking the next. Also read any text the user attached when invoking this skill — if it answers a question, skip it.

**Q1 — Purpose** _(skip if README clearly states it)_:
> "In one sentence: what does this project do, and who uses it?"

**Q2 — Current focus** _(always ask unless a ROADMAP or open issues make it obvious)_:
> "What are you actively working on or trying to accomplish next in this project?"

**Q3 — Gotchas** _(only if the code suggests non-obvious constraints)_:
> "Any non-obvious conventions or constraints I should document? E.g. 'always register routes manually in X', 'image keys must match exactly'."

Skip any question you already have the answer to from reading the code or user-provided text.

---

## Step 4: Write ABOUT.md

Create `.claude/context/` if it doesn't exist, then write or update `ABOUT.md`:

```markdown
# About

_Last updated: {date}_

## What It Is

{one sentence: what the project does and who uses it}

## Stack

- **Framework:** {name and version}
- **Language:** {language}
- **Build tool:** {tool}
- **Deployment:** {how/where, if determinable}

## Structure

{annotated directory tree — only directories that matter, one-line explanations each}

## Key Files

| File | Purpose |
|------|---------|
| {path} | {what it is} |

## Common Tasks

- **{task name}:** touch `{file A}`, then `{file B}` — {one-line recipe}

## Conventions & Gotchas

- {non-obvious rule or constraint}

## Key Documentation

{Only include if a docs folder or notable markdown files exist. Summarize the most important points from each — architecture decisions, data models, setup requirements, workflow rules, anything a developer must know. Do not just list file names; extract the substance.}
```

**Quality rules:**
- Every file path must be real and verified from what you read
- Every field must reflect actual current data — read the real files
- No generic advice — only project-specific facts
- Keep under 100 lines

---

## Step 5: Write PLAN.md

Write or update `.claude/context/PLAN.md`:

```markdown
# Plan

_Last updated: {date}_

## Current Phase

{one sentence describing where the project is right now — e.g. "Initial scaffolding", "Feature buildout", "Pre-launch polish", "Maintenance"}

## Goals

{what the project is trying to achieve at this stage — 2–4 bullet points}

## Steps

Ordered tasks to reach the current goals. Update status as work progresses.

| # | Task | Status | Notes |
|---|------|--------|-------|
| 1 | {task} | {Todo \| In Progress \| Done \| Blocked} | {optional context} |

## Completed

{tasks fully done and no longer needing attention — move here from Steps when done}

- {task} — {brief note on outcome, if useful}

## Blockers

{anything preventing progress — remove when resolved}

- {blocker} — {what's needed to unblock}

## Open Questions

{unresolved decisions or unknowns that will affect the plan}

- {question}
```

**Quality rules:**
- Tasks must be specific and actionable, not vague ("Add auth" not "Improve security")
- Derive tasks from: user's stated focus (Q2), visible TODOs in code, incomplete scaffolding, and ROADMAP/CHANGELOG if present
- If the project is early-stage, focus on foundation tasks; if mature, focus on what's visibly in-progress
- Keep under 60 lines

---

## Step 6: Confirm

```
✓ .claude/context/ABOUT.md {created|updated}
✓ .claude/context/PLAN.md {created|updated}

Run /cload at the start of any session to orient Claude on this project.
Run /csync after a session to keep both files current.
```

---

## Notes

- Never document things you haven't verified by reading actual files.
- Ask questions one at a time. Do not batch them.
- If the project has changed significantly since a previous ABOUT.md, rewrite affected sections fully rather than patching stale content.
- PLAN.md is meant to be a living document — it should reflect what's actually happening, not an idealized roadmap.