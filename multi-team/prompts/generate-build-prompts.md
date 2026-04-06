# Generate Build Prompts

A reusable workflow for generating structured build prompts for any project phase.

Ported from SUPWISE's `generate-phase-prompt.md`, generalized for any project.

---

## Overview

This workflow takes a **phase spec** (feature description, PRD, or design doc) and produces a complete set of **build prompts** — structured instructions that agents can execute sequentially to implement the phase.

The Orchestrator drives this workflow by delegating to all three teams.

---

## Step 1 — Parse & Init

**Owner:** Orchestrator

1. Identify the phase/feature to build
2. Locate the relevant spec, PRD, or design doc
3. Check if build prompts already exist (overwrite or extend?)
4. Check if a tracker exists; create one if not

**Output:** Phase identifier + spec location + decision on overwrite/extend

---

## Step 2 — Parallel Context Gathering

**Owner:** Planning Lead → delegates to workers in parallel

### Architect: Spec Analysis
- Read the phase spec / PRD / design doc
- Read project architecture docs and decision records
- Read related module specs
- **Output:** Summary per document — key deliverables, endpoints, data model, open questions, deferred items

### Architect: Reference Analysis
- Read previous phase's build prompts (structure reference)
- Read the "golden standard" prompts if one exists
- Check previous phase completion status (prerequisites)
- **Output:** Prompt structure pattern, quality bar, prerequisite status

### Spec Writer: Codebase Scan
- Scan existing code — which modules, routes, components exist?
- Read 1 controller + 1 service as pattern reference
- Read DB schema for relevant models
- **Output:** Existing patterns (controller/service/DTO/component structure), what already exists

### Spec Writer: Skills & Rules Extraction
- Read project-specific skill files and best practices
- Read checklists (backend, frontend, security)
- **Output:** Top-10 most impactful rules per skill for building a new module

### Validation Lead → QA Engineer: Lessons Learned
- Read past code reviews and security reports
- Read past bug reports and incident logs
- Extract repeatable bug patterns
- **Output:** Repeatable-bug checklist — concrete items for every prompt

**Gate:** All 5 context streams must complete before proceeding.

---

## Step 3 — Design Decisions

**Owner:** Orchestrator (interactive with user)

After receiving all context:

1. **Analyze** the spec for open questions, conflicts, and practical concerns:
   - Data model mismatches (nullable vs NOT NULL, missing fields)
   - Unanswered spec questions
   - Features that sound good on paper but are hard in practice
   - Missing data mapping tables (status enums, code mappings)

2. **Check frontend→backend alignment:**
   - Does a modal need a preview/validation step? → backend preview endpoint needed?
   - Does a wizard have multiple steps? → interim storage endpoints?
   - Is there a confirmation screen? → separate GET endpoint for dry-run/preview?

3. **Apply standard patterns** (auto-decisions unless not applicable):
   - **Navigation:** new pages → sidebar/nav menu items
   - **Optimistic locking:** multi-user write endpoints → `expectedUpdatedAt` + 409 Conflict
   - **URL state sync:** list pages with filters → search params sync
   - **Accessibility:** interactive elements → build a11y in, not just review

4. **Categorize each decision:**
   - **AUTO:** clear choice based on lessons learned, existing patterns, best practices. Mark with "(Auto-selected: {reason})"
   - **OPEN:** real choice requiring user input. Present options with implementation impact.

5. **Present decisions one at a time** — AUTO first (quick confirm), OPEN after (more thought needed)

6. **Summary table** after all decisions — all D-numbers + choice + implementation impact

**Gate:** User confirms all decisions before prompt generation.

---

## Step 4 — Parallel Prompt Generation

**Owner:** Engineering Lead → coordinates, Orchestrator delegates

### Backend Prompts (Engineering Lead → Backend Dev)

Generate prompts for:
- **Prompt 1:** Database migration + schema
- **Prompt 2A-2C:** Backend modules/features (CRUD, business logic, integrations)
- **Prompt 3B:** Backend code review
- **Prompt 3A:** API tests (on reviewed code)

Each prompt includes:
- Skills to load (exact paths)
- Context files (concrete paths)
- Out of scope (what NOT to build)
- Deliverables per layer (method names, types, fields — not vague)
- Rules (pattern references to existing code)
- Failure states (what does the user see when X fails?)
- Repeatable-bug check (from Step 2 lessons learned)
- Build check command
- Output Contract (files, exports, module registration)
- Rollback strategy (git stash + checkout)
- Agent team assignment (max 4, as dependency DAG not linear list)

### Frontend Prompts (Engineering Lead → Frontend Dev)

Generate prompts for:
- **Prompt 4A:** Types, services, API client
- **Prompt 4B:** UI components + pages
- **Prompt 4B2:** Complex UI (editable grids, drag-drop, wizards) — split if needed
- **Prompt 4C:** Frontend code review
- **Prompt 4D:** E2E tests (ALWAYS last)

Same required sections as backend, plus:
- Navigation updates (sidebar, breadcrumbs)
- Accessibility requirements (aria-labels, focus management, keyboard nav)
- URL state sync for filterable pages

### Security Prompt (Validation Lead → Security Reviewer)

Generate prompt for:
- **Prompt 5A:** Security audit

Includes:
- OWASP top 10 specific to this phase (not generic)
- Auth/authorization audit table
- Known issues to verify from lessons learned
- Dependency audit commands

**Gate:** All 3 prompt streams must complete.

---

## Step 5 — Assemble & Self-Validate

**Owner:** Planning Lead → Spec Writer

### 5A: Assemble Document

Combine all outputs into `specs/phase-{N}-prompts.md`:

```markdown
# Phase {N} — Build Prompts ({Name})

Generated: {DATE}

## Design Decisions (confirmed)
{From Step 3}

## Standard Error Pattern
{Generated from existing code patterns}

---
## Prompt 1: Database Migration
## Prompt 2A-2C: Backend
## Prompt 3B: Backend Code Review
## Prompt 3A: API Tests
---
## Prompt 4A-4B2: Frontend
## Prompt 4C: Frontend Code Review
---
## Prompt 5A: Security Audit
---
## Prompt 4D: E2E Tests (ALWAYS LAST)
---
## Prompt Execution Order
{Numbered overview with dependencies}
```

### 5B: Self-Validate

**Owner:** Validation Lead → QA Engineer

Before presenting to user, validate:

1. **Output contract completeness** — every code-prompt has an output contract
2. **Dependency consistency** — exports from prompt N match imports of prompt N+1, no circular deps
3. **Scope coverage** — every endpoint/feature from the spec appears in at least 1 prompt
4. **Numbering** — no duplicate or missing prompt numbers
5. **Decision references** — every D-number referenced exists, every D-number defined is referenced
6. **Section completeness** — every prompt has all required sections

**If validation fails:** fix before presenting. Log what was found and fixed.

---

## Step 6 — Deliver & Track

**Owner:** Orchestrator

1. Present the complete prompts document to the user
2. Show: design decisions + prompt titles + total count
3. Update tracker with decisions and prompt status
4. Ask: "Prompts generated. Ready to start with Prompt 1?"

---

## Execution Order (mandatory)

```
Backend build:     1 → 2A → 2B → 2C
Backend review:    3B (code review) → 3A (API tests on reviewed code)
Frontend build:    4A → 4B → 4B2
Review + Security: 4C (FE code review) → 5A (security audit)
E2E tests:         4D (ALWAYS LAST)
```

**Why review BEFORE tests:** Code review changes DTOs, service signatures, error handling, and null-safety. Tests written on pre-review code break after review fixes. Tests on reviewed code = no rework.

---

## Quality Rules (per prompt)

### Mandatory Sections
- [ ] Skills (exact paths)
- [ ] Context (concrete file paths)
- [ ] Out of scope
- [ ] Deliverables per layer (concrete names, types, fields)
- [ ] Rules (pattern references)
- [ ] Failure states (what user sees when X fails)
- [ ] Repeatable-bug check
- [ ] Build check command
- [ ] Output Contract (files, exports, module status)
- [ ] Rollback strategy
- [ ] Agent teams as DAG

### Structural Rules
- Max ~150 lines per prompt — split if larger
- Complex UI in separate prompt (4B2)
- Tests always separate (3A, 4D)
- Security always separate (5A)
- 1 commit per prompt — atomic, revertable units
- Never commit if build fails

### Cross-Cutting Concerns
- **Validation parity** — create + edit forms use same validators
- **Edge case exhaustivity** — all states including cancelled/archived/empty
- **No ambiguities** — auto-resolve technical choices from existing patterns
- **Preview endpoints** — if frontend has preview/confirm before mutation, ensure backend endpoint exists
- **Rate limiting** — CPU/IO-heavy endpoints always get throttling
- **Optimistic locking** — multi-user write endpoints get `expectedUpdatedAt` + 409
- **Accessibility** — build it in (4B, 4B2), don't defer to review (4C)

---

## Team Responsibility Map

| Step | Orchestrator | Planning | Engineering | Validation |
|------|-------------|----------|-------------|------------|
| 1. Parse & Init | ✅ owns | | | |
| 2. Context Gather | delegates | ✅ leads | | ✅ lessons |
| 3. Design Decisions | ✅ owns | advises | | |
| 4. Prompt Generation | delegates | | ✅ leads | ✅ security |
| 5A. Assemble | | ✅ spec writer | | |
| 5B. Validate | | | | ✅ QA |
| 6. Deliver & Track | ✅ owns | | | |
