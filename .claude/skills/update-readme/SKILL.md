---
name: update-readme
description: Use when someone asks to update the readme, reflect changes in the docs, sync documentation with code, or when readme needs updating.
argument-hint: [description of what changed]
---

## What This Skill Does

Keeps the README in sync with code changes. Identifies only the affected sections, proposes targeted updates that preserve the existing tone and formatting, and waits for approval before writing anything.

## Steps

### 1. Read the README

Find and read the README file. Check for `README.md`, `README.rst`, or `docs/README.md` in the project root. If none found, ask the user where it is.

Note the existing:
- Overall structure and section names
- Tone (formal, casual, technical)
- Formatting conventions (code block style, heading levels, list style, etc.)

### 2. Get the Changes

If `` was provided, use it as the description of what changed.

Otherwise, run `git diff HEAD` to get recent unstaged changes. If that's empty, try `git diff HEAD~1` to get the last commit's diff. If still empty, ask the user to describe what changed.

### 3. Identify Affected Sections

Based on the changes, determine which README sections need updating. Common mappings:
- New/changed CLI flags or function signatures → **Usage**, **API Reference**
- New dependencies or install steps → **Installation**, **Requirements**
- New env vars or config options → **Configuration**, **Environment Variables**
- Changed default behavior → **Usage**, **Overview**, **How It Works**
- New features → **Features**, **Usage**
- Removed features → any section that references them

List the affected sections explicitly before proposing any changes.

### 4. Propose Updates

For each affected section, show:
- The **section name**
- The **proposed new content** (full replacement for that section only)

Format your proposal clearly so the user can compare. Match the existing tone and formatting exactly — do not restructure, reformat, or improve other parts of the README.

If a section doesn't exist yet but should be added, show where it would be inserted and its full content.

### 5. Wait for Approval

After presenting all proposed changes, ask the user:

> "Does this look good? I'll apply these changes once you confirm."

Do NOT write anything to the README until the user approves. If the user requests edits, revise and re-present before asking again.

### 6. Apply Changes

Once approved, use Edit to apply each change to the README. Update only the approved sections. Do not touch anything else.

Confirm when done: "README updated — [list of sections changed]."

## Notes

- Never rewrite sections unrelated to the code changes, even if they look outdated.
- Never "improve" tone, fix typos, or restructure unprompted — only change what the diff requires.
- If the diff is large and touches many areas, list all affected sections and ask the user if they want to proceed with all of them or a subset.
- If the README has no clear sections, treat the whole document as one section and propose a full replacement — but still wait for approval.
