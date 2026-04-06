# Phased Development Workflow

A four-phase workflow for feature development with context-first approach.

## Overview

Every non-trivial feature goes through four phases:

1. **Context** --- gather project context before anyone starts
2. **Plan** --- understand what to build and how
3. **Build** --- implement, test (E2E), and review the solution
4. **Validate** --- verify it works and is safe

The [[orchestrator]] drives this workflow by delegating at each phase.
Each phase reports back to the orchestrator before the next phase starts.

---

## Phase 1: Context

**Owner:** [[context-loader|Context Loader]] (standalone utility agent)

**Input:** User request or feature description

**Steps:**
1. Orchestrator delegates to Context Loader with the user's task description
2. Context Loader scans project structure, relevant files, dependencies
3. Context Loader produces a structured Context Report
4. Context Report is passed to all downstream phases

**Output:**
- Structured context report (project structure, relevant files, patterns, risks)

**Gate:** Orchestrator reviews the context report. If sufficient, proceed to Plan.
If context is unclear or task is ambiguous, ask the user for clarification.

---

## Phase 2: Plan

**Owner:** [[planning/lead|Planning Lead]]

**Input:** User request + Context Report from Phase 1

**Steps:**
1. Orchestrator delegates to Planning Lead with user context AND the Context Report
2. Planning Lead routes to [[planning/architect|Architect]] for system design
3. Planning Lead routes to [[planning/spec-writer|Spec Writer]] for detailed spec
4. Planning Lead reviews and reports back to Orchestrator

**Output:**
- Architecture decision record in `specs/`
- Detailed specification in `specs/`
- Risk assessment and open questions

**Gate:** Orchestrator reviews the plan. If acceptable, proceed to Build.
If not, provide feedback and re-delegate to Planning.

---

## Phase 3: Build

**Owner:** [[engineering/lead|Engineering Lead]]

**Input:** Approved specs from Phase 2 + Context Report from Phase 1

**Steps:**
1. Orchestrator delegates to Engineering Lead with spec references + context
2. Engineering Lead routes to [[engineering/backend-dev|Backend Dev]] for server-side code
3. Engineering Lead routes to [[engineering/frontend-dev|Frontend Dev]] for client-side code
4. Engineering Lead routes to [[engineering/playwright-tester|Playwright Tester]] for E2E tests
5. Engineering Lead routes to [[engineering/code-reviewer|Code Reviewer]] for code review
6. Engineering Lead reviews all output and reports back to Orchestrator

**Build sub-phases:**

### 3a. Implement
Backend Dev and Frontend Dev work (in parallel if independent, or backend-first for API contracts).

### 3b. E2E Test
Playwright Tester writes and runs E2E tests against the implementation.
If tests fail, Engineering Lead routes fixes back to the relevant dev.

### 3c. Code Review
Code Reviewer reviews all changes (implementation + tests).
If review finds Critical/Major issues, Engineering Lead routes fixes back to devs.

**Output:**
- Implementation code in `src/`
- Unit tests in `tests/`
- E2E tests in `tests/e2e/`
- Code review report (verdict: APPROVE / REQUEST CHANGES)
- Summary of what was built and any spec deviations

**Gate:** Orchestrator reviews the build summary + code review verdict.
If review is APPROVE and tests pass, proceed to Validate.
If REQUEST CHANGES, route fixes back to Engineering.

---

## Phase 4: Validate

**Owner:** [[validation/lead|Validation Lead]]

**Input:** Implemented code from Phase 3 + specs from Phase 2

**Steps:**
1. Orchestrator delegates to Validation Lead with file paths and specs
2. Validation Lead routes to [[validation/qa-engineer|QA Engineer]] for testing
3. Validation Lead routes to [[validation/security-reviewer|Security Reviewer]] for security audit
4. Validation Lead synthesizes findings and reports back to Orchestrator

**Output:**
- Test results (pass/fail/coverage)
- Security findings with severity ratings
- Recommended fixes (if any)

**Gate:** Orchestrator reviews validation results. If all clear, report
success to user. If issues found, route fixes back to Engineering (Phase 3).

---

## Flow Diagram

```
User Request
    |
    v
[Phase 1: CONTEXT]  ──> Context Loader
    |
    v  (context report)
[Phase 2: PLAN]  ──> Planning Lead ──> Architect + Spec Writer
    |
    v  (specs + architecture)
[Phase 3: BUILD]  ──> Engineering Lead
    |                   ├── Backend Dev    ──┐
    |                   ├── Frontend Dev   ──┤ (3a: implement)
    |                   ├── Playwright Tester (3b: E2E test)
    |                   └── Code Reviewer    (3c: review)
    |
    v  (code + tests + review)
[Phase 4: VALIDATE]  ──> Validation Lead ──> QA + Security
    |
    v
Done (or loop back to Phase 3 for fixes)
```

---

## Shortcuts

Not every task needs all four phases:

| Task Type | Phases | Flow |
|-----------|--------|------|
| Architecture/design | Context + Plan | Context Loader → Architect + Spec Writer |
| Bug fix | Context + Build | Context Loader → Backend/Frontend Dev → Review |
| Code review only | Context + Build (review) | Context Loader → Code Reviewer |
| E2E test only | Context + Build (test) | Context Loader → Playwright Tester |
| Full feature | All four | Context → Plan → Build → Validate |
| Investigation | Context only | Context Loader |
| Security audit | Context + Validate | Context Loader → QA + Security |

The Orchestrator decides which phases are needed based on the task.
**Context phase is always included** — every task benefits from context loading.

---

## Session Logging

Each phase should log its outcomes to `{{session_dir}}` so the conversation
log captures the full history of decisions and deliverables.
