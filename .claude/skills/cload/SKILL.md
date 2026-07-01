---
name: cload
description: Load project context at the start of a session. Reads .claude/context/ABOUT.md and .claude/context/PLAN.md so Claude understands the project's purpose, stack, structure, conventions, and current plan. If ABOUT.md is missing, directs to /cinit. If PLAN.md is missing, offers to create it.
---

## What This Skill Does

Orients Claude at the start of a session by reading `.claude/context/ABOUT.md` and `.claude/context/PLAN.md`. Mostly read-only — if PLAN.md is missing, offers to create it with a few quick questions. One short confirmation output.

---

## Step 1: Check for ABOUT.md

Look for `.claude/context/ABOUT.md`.

**If missing:**
> "No context file found at `.claude/context/ABOUT.md`. Run `/cinit` to create one."

Stop here.

**If present:** Continue to Step 2.

---

## Step 2: Read ABOUT.md

Read `.claude/context/ABOUT.md` in full. Internalize:
- What the project is and who uses it
- The tech stack
- Directory structure and what each part does
- Key files and their roles
- Common tasks and how they're done
- Conventions and gotchas

---

## Step 3: Check for PLAN.md

Look for `.claude/context/PLAN.md`.

**If present:** Read it in full. Internalize:
- Current phase and goals
- What's in progress, what's todo, what's blocked
- Completed work and open questions

Continue to Step 4.

**If missing:** Ask one question:
> "No plan file found at `.claude/context/PLAN.md`. What are you working on or trying to accomplish in this session?"

Then create PLAN.md using the answer plus what you can infer from ABOUT.md and a quick scan of the repo (visible TODOs, incomplete scaffolding, ROADMAP/CHANGELOG if present). Use the same PLAN.md format defined in `/cinit`. Confirm:
> "✓ `.claude/context/PLAN.md` created"

Continue to Step 4.

---

## Step 4: Output Orientation Summary

One concise block — enough to confirm orientation, not a full readout:

```
Context loaded: {project name}

What it is: {one sentence from ABOUT.md}
Stack: {framework / language}

Current phase: {phase from PLAN.md}
Up next: {the first In Progress or Todo task from PLAN.md}
```

If PLAN.md was just created, omit "Up next" and instead show what was captured:
```
Plan created: {current phase} — {number} tasks logged
```

---

## Notes

- This skill is read-only except for creating a missing PLAN.md.
- Keep the output short. The goal is orientation, not a summary of everything in the files.
- After loading, apply the conventions, project knowledge, and plan context naturally in all subsequent responses during the session.
- If the plan looks stale (tasks marked In Progress that seem long-finished based on the codebase), flag it:
  > "Note: PLAN.md may be out of date — run `/csync` to refresh it."