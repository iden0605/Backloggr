---
name: csync
description: Sync .claude/context/ABOUT.md and .claude/context/PLAN.md after a session where the codebase changed or work progressed. Reviews what was modified during the conversation and updates only the drifted sections. Run at the end of a session to keep context current for next time.
---

## What This Skill Does

Keeps `.claude/context/ABOUT.md` and `.claude/context/PLAN.md` accurate after sessions where the codebase changed or work progressed. Reviews changes made during the conversation, re-reads affected files, and updates only the sections that have drifted — no full rewrite, no unnecessary questions.

---

## Step 1: Check for Context Files

Check whether `.claude/context/ABOUT.md` and `.claude/context/PLAN.md` exist.

**If ABOUT.md is missing:**
> "No context file found at `.claude/context/ABOUT.md`. Run `/cinit` to create one."

Stop here.

**If PLAN.md is missing:**
> "No plan file found at `.claude/context/PLAN.md`. Run `/cload` or `/cinit` to create one."

Continue to sync ABOUT.md only (Steps 2–6 for ABOUT.md, skip Steps 7–9).

**If both exist:** Continue to Step 2.

---

## Step 2: Read Both Files

Read `.claude/context/ABOUT.md` and `.claude/context/PLAN.md` fully. Note:
- Which files and paths ABOUT.md references
- Which tasks in PLAN.md are marked Todo, In Progress, or Blocked

---

## Step 3: Review Session Changes

Look back at what was modified or accomplished in this conversation:

**For ABOUT.md — codebase changes:**
- Which files were edited or created?
- Were any directories added or removed?
- Did any dependencies, scripts, or config change?
- Were any new patterns, routes, or conventions introduced?

**For PLAN.md — progress changes:**
- Were any tasks completed, started, or blocked during this session?
- Did the scope or direction shift?
- Were any new tasks identified that aren't in PLAN.md?
- Were any open questions resolved?

Also re-read the key files that `ABOUT.md` documents to check for drift. If a `docs/`, `documentation/`, `doc/`, or `wiki/` directory exists, check for any new or modified files and incorporate changes into the **Key Documentation** section.

---

## Step 4: Identify Drift

List what has actually changed vs what each file documents. Be specific:

**ABOUT.md drift:**
- New file or directory not documented
- A route, component, or module added/removed
- A dependency or script changed
- A convention or gotcha that's now outdated

**PLAN.md drift:**
- A task was completed → move to Completed
- A task was started → mark In Progress
- A new task emerged → add to Steps
- A blocker was resolved → remove from Blockers
- A new blocker appeared → add to Blockers
- An open question was answered → remove or update it

**If nothing has changed in either file:**
> "Both files are current — no changes needed."

Stop here.

---

## Step 5: Ask One Question if Needed

If changes in the code or conversation suggest something you can't determine from reading alone (e.g. a new module whose purpose isn't clear, or whether a task is truly done vs paused), ask one targeted question:
> "I see `{new thing}` was added — what does it do?" or "Is `{task}` fully done or just paused?"

Do not ask about changes you can determine from reading the code or conversation.

---

## Step 6: Update ABOUT.md

Edit only the sections that have drifted. Do not rewrite sections that are still accurate. Update the `_Last updated_` date.

Before writing, state what you're changing:
> "Updating ABOUT.md: {list of changed sections}"

---

## Step 7: Update PLAN.md

Edit only what has changed:
- Move completed tasks from **Steps** to **Completed**
- Update status fields (Todo → In Progress → Done)
- Add new tasks in their correct order
- Remove resolved blockers / add new ones
- Remove answered open questions / add new ones
- Update **Current Phase** if the project has moved to a new stage
- Update the `_Last updated_` date

Before writing, state what you're changing:
> "Updating PLAN.md: {list of changes — e.g. 'marked task 3 Done, added task 6, removed blocker'}"

---

## Step 8: Confirm

```
✓ .claude/context/ABOUT.md synced
✓ .claude/context/PLAN.md synced

ABOUT.md — Updated: {changed sections} | Unchanged: {stable sections}
PLAN.md  — {tasks completed: N} | {tasks added: N} | {status changes: N}
```

If only one file changed, report only that one and note the other was already current.

---

## Notes

- Do not ask questions the code or conversation can answer. Minimize friction.
- If the project has changed significantly (new framework, major restructure), recommend `/cinit` instead of patching a deeply stale `ABOUT.md`.
- Only update what has actually changed — preserve accurate sections exactly as they are.
- PLAN.md drift is often more frequent than ABOUT.md drift — expect to update it most sessions.
- If PLAN.md tasks look fundamentally wrong (misaligned with what actually happened), note it:
  > "PLAN.md seems significantly out of date. Consider running `/cinit` to rebuild the plan from scratch."