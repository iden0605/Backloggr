---
name: write-docs
description: Use when someone asks to document a change, write docs for a feature, write documentation for this, or document what I just did.
argument-hint: [description of the change to document]
---

## What This Skill Does

Reads the git diff, identifies the relevant change, and produces a developer-facing markdown doc covering what changed, why it exists, and how to use it — then waits for approval before writing the file.

## Steps

### 1. Read Existing Docs for Style Reference

Check if a `/docs` directory exists. If it does, read 1-2 existing files to establish the tone, formatting conventions, and heading structure used. Note these for use in Step 4.

### 2. Get the Diff

Run `git diff HEAD` to get unstaged changes. If empty, run `git diff HEAD~1` to get the last commit. Also run `git log -1 --pretty=%B` to get the most recent commit message.

### 3. Identify the Relevant Change

Use  (if provided) or the commit message to identify which part of the diff to document. If the diff contains multiple distinct changes and it's ambiguous which one the user means, ask before proceeding.

The relevant change is what drives the entire document — don't document unrelated parts of the diff.

### 4. Draft the Documentation

Produce a markdown document with this structure:

```
# [Feature or Change Name]

## Overview
[1-2 sentence explanation of what this is and what problem it solves]

## What Changed
[Concise description of the change — new function, new flag, new config option, etc.]

## Why It Exists
[The motivation — derive from the commit message, user's description, or code context. If unclear, note that and ask.]

## Usage

[Code example showing how to use the feature]

## [Additional sections as needed — e.g. Configuration, Parameters, Caveats]
```

Rules:
- Match the tone and formatting of existing docs if they exist
- The code example must be concrete and runnable, not pseudocode
- Do NOT invent the "why" — derive it from available context or flag it as unclear
- Keep it developer-facing: assume the reader can read code but needs guidance on intent and usage

### 5. Propose and Wait for Approval

Present the full draft to the user. Tell them the file will be written to `/docs/[feature-name].md`.

Ask: "Does this look good? I'll write the file once you confirm."

Do NOT write the file until the user approves. If they request edits, revise and re-present.

### 6. Write the File

Derive the filename from the feature name: lowercase, hyphens, `.md` extension (e.g. `verbose-flag.md`, `auth-middleware.md`).

Check if `/docs/[filename].md` already exists. If it does, warn the user before overwriting.

Write the approved content to `/docs/[feature-name].md`.

Confirm: "Written to `/docs/[feature-name].md`."

## Notes

- If there's no diff at all (clean working tree, no recent commits), ask the user to describe the change or point to a specific commit.
- Don't create a `/docs` directory unless the user confirms they want one.
- If "why it exists" is genuinely unclear from all available context, write a placeholder and flag it explicitly rather than guessing.
- One doc per change. If the user wants to document multiple changes, handle them sequentially.
