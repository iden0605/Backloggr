---
name: ui-review
description: Use when someone asks to review their UI, audit their frontend design, improve their website's design or layout, suggest UI/UX improvements, or enhance the visual styling of their frontend. Also triggers on phrases like "review my UI", "improve my frontend design", "audit my design", or "make my website look better".
argument-hint: [page, component, directory, or "all"]
---

## What This Skill Does

Acts as a UI/UX design manager. Analyzes the frontend codebase, engages the user in a conversation about design improvements, agrees on a cohesive visual direction, then orchestrates multiple `/frontend-design` subagents in parallel — one per component or page — to apply improvements with a consistent, unified aesthetic.

## Step-by-Step Workflow

### Step 1: Detect the Frontend Stack

Read the repo structure to identify:
- Framework (React, Vue, Next.js, Svelte, plain HTML, etc.)
- Styling approach (CSS modules, Tailwind, styled-components, SCSS, etc.)
- Entry points and routing structure
- Component and page directories

Report findings to the user briefly before proceeding.

### Step 2: Determine Scope

If `` was provided, use it as the scope. Otherwise ask the user:

> "What should I focus on? You can name a specific page, component, or directory — or say **all** to review the entire frontend."

If the user says "all", collect all frontend component and page files. Exclude:
- Backend files, API routes, server-side logic
- Framework configuration files (next.config.js, vite.config.ts, etc.)
- Backend-frontend communication layers (fetch wrappers, API clients, tRPC routers, etc.)
- Test files

### Step 3: Request Screenshots (Optional)

Ask the user:

> "Do you have any screenshots of the current UI? Sharing them will help me give better suggestions — but it's optional."

Accept images if provided. If not, proceed with code analysis only.

### Step 4: Analyze the Frontend Code

Read all files within scope. For each component/page, evaluate:
- Visual hierarchy and layout structure
- Typography choices (font families, sizes, weights, spacing)
- Color palette and consistency
- Spacing, padding, and alignment
- Motion and interactivity (animations, transitions, hover states)
- Design cohesion across pages
- Any obvious UX friction points

Identify patterns, inconsistencies, and the biggest opportunities for improvement.

### Step 5: Present Findings and Discuss

Present your analysis in a structured format:

```
## UI/UX Analysis

**Stack:** [detected stack]
**Scope:** [files/pages in scope]

### What's Working
- [positive observations]

### Improvement Opportunities
1. [issue + suggestion]
2. [issue + suggestion]
...

### Proposed Design Direction
[Describe a cohesive aesthetic direction — tone, color palette concept, typography direction, layout approach, motion philosophy]
```

Then open a conversation with the user. Ask:
- Do they agree with the direction?
- Any constraints (brand colors, accessibility requirements, must-keep elements)?
- Preferences on tone (minimal, bold, playful, corporate, etc.)?

Iterate until you and the user agree on a **single cohesive design brief** that will be shared across all subagents. The brief must specify:
- Aesthetic direction and tone
- Color palette (primary, accent, background, text)
- Typography direction (display font style, body font style)
- Motion/animation philosophy
- Layout and spacing philosophy
- Any must-keep elements

### Step 6: Confirm Before Applying

Present the final design brief to the user and ask for explicit confirmation:

> "Here's the design brief I'll apply across all components. Ready to proceed? This will edit files in place."

Do NOT proceed until the user confirms.

### Step 7: Orchestrate Subagents

Once confirmed, split the scope into individual units (one per component/page file or logical grouping). Launch one subagent per unit using the Agent tool in parallel.

**Each subagent prompt must include:**

```
You are a frontend design agent. Your task is to improve the UI/UX of a specific frontend file.

**Design Brief (apply consistently across all changes):**
[paste the full agreed design brief here]

**File to improve:** [file path]

**Current file contents:**
[paste file contents]

**Instructions:**
- Apply the design brief to this file
- Improve visual design, layout, typography, spacing, and motion as appropriate
- Do NOT change any component logic, props, data fetching, or backend communication
- Do NOT modify framework configuration
- Write the improved code back to the file in place
- Match the aesthetic direction exactly — this file must look cohesive with all other pages

Use the frontend-design skill guidelines: distinctive typography, intentional color use, spatial composition, and memorable visual details.
```

Monitor all subagents. Once all complete, report back to the user with a summary of what was changed.

### Step 8: Report Completion

After all subagents finish:
- List all files modified
- Summarize the key design changes applied
- Note any files that were skipped or had issues

## Notes

**What this skill NEVER touches:**
- Backend code, API routes, server functions
- Database queries or ORM logic
- Backend-frontend communication (fetch clients, API wrappers, tRPC, GraphQL schemas)
- Framework configuration files
- Test files

**Subagent parallelism:** All subagents run in parallel via the Agent tool. Pass the full design brief to every subagent so the output is visually cohesive.

**Large codebases:** If scope is "all" and the repo has many files, group related components together per subagent rather than one subagent per file to keep the number manageable (aim for 5–15 subagents max).

**Design brief is the source of truth:** Every subagent receives the same brief. The manager (this skill) is responsible for ensuring the brief is specific enough that independently-run subagents produce cohesive output. If the brief is vague, the output will be inconsistent — iterate with the user until it's precise.

**No auto-applying:** Never apply changes without explicit user confirmation in Step 6.
