# Phase-Based Development Workflow Architecture

**Author:** Architect  
**Date:** 2026-04-06  
**Status:** Draft  
**Supersedes:** `prompts/plan-build-validate.md` (single-pass workflow)

---

## 1. Executive Summary

This document defines a **phase-based development workflow** that replaces the current single-pass plan-build-validate model. The key changes are:

1. **Multi-phase decomposition** — large tasks are broken into sequential phases, each self-contained with its own plan-build-validate cycle.
2. **Context Loader agent** — a new standalone, read-only agent that gathers codebase state and prior phase outputs before each phase begins.
3. **Expanded Engineering team** — adds a **Playwright Tester** (E2E tests) and a **Code Reviewer** (quality gate) to the existing Backend Dev and Frontend Dev.
4. **Per-phase validation** — the Validation team runs at the end of every phase (not just as a final gate), catching issues incrementally.
5. **Structured phase artifacts** — each phase produces outputs in a well-defined directory, enabling clean handoffs between phases.

The design fits within the existing pi multi-team framework (orchestrator → leads → workers) with no structural breaking changes — only additions.

---

## 2. Workflow Diagram

### 2.1 Per-Phase Pipeline (Single Phase)

```
┌─────────────────────────────────────────────────────────────────────┐
│                         PHASE N                                     │
│                                                                     │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────────┐    │
│  │   STEP 1:    │     │   STEP 2:    │     │     STEP 3:      │    │
│  │   CONTEXT    │────▶│   PLANNING   │────▶│   ENGINEERING    │    │
│  │   LOADING    │     │   TEAM       │     │     TEAM         │    │
│  │              │     │              │     │                  │    │
│  │ Context      │     │ Planning     │     │ Engineering      │    │
│  │ Loader       │     │ Lead         │     │ Lead             │    │
│  │ (standalone) │     │  ├─Architect │     │  ├─Backend Dev   │    │
│  │              │     │  └─Spec      │     │  ├─Frontend Dev  │    │
│  │              │     │    Writer    │     │  ├─Playwright    │    │
│  │              │     │              │     │  │  Tester       │    │
│  │              │     │              │     │  └─Code Reviewer │    │
│  └──────┬───────┘     └──────┬───────┘     └────────┬─────────┘    │
│         │                    │                      │              │
│         │ context.md         │ phase-plan.md        │ impl report │
│         │                    │ phase-spec.md        │ + code      │
│         ▼                    ▼                      ▼              │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                     ORCHESTRATOR                             │   │
│  │              (receives output from each step,                │   │
│  │               passes to next step as input)                  │   │
│  └──────────────────────────┬──────────────────────────────────┘   │
│                             │                                      │
│                             ▼                                      │
│                    ┌──────────────┐                                 │
│                    │   STEP 4:    │                                 │
│                    │  VALIDATION  │                                 │
│                    │   TEAM       │                                 │
│                    │              │                                 │
│                    │ Validation   │                                 │
│                    │ Lead         │                                 │
│                    │  ├─QA        │                                 │
│                    │  │ Engineer  │                                 │
│                    │  └─Security  │                                 │
│                    │   Reviewer   │                                 │
│                    └──────┬───────┘                                 │
│                           │                                        │
│                           ▼                                        │
│                    ┌──────────────┐                                 │
│                    │ PHASE GATE   │                                 │
│                    │ (Orchestrator│                                 │
│                    │  decides)    │                                 │
│                    └──────┬───────┘                                 │
│                           │                                        │
└───────────────────────────┼────────────────────────────────────────┘
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
         ┌────────┐   ┌────────┐   ┌──────────┐
         │ PASS → │   │ REWORK │   │  ABORT   │
         │ Next   │   │ Re-run │   │  Report  │
         │ Phase  │   │ step   │   │  to user │
         └────────┘   └────────┘   └──────────┘
```

### 2.2 Multi-Phase Loop (Full Task Lifecycle)

```
  User Request
       │
       ▼
  ┌──────────┐     ┌──────────────────────────────────┐
  │Orchestr- │     │  PHASE DECOMPOSITION              │
  │ator      │────▶│  Break task into N phases          │
  │          │     │  Write to phases/manifest.md       │
  └──────────┘     └─────────────┬────────────────────┘
                                 │
                                 ▼
                   ┌─────────────────────────┐
                   │      PHASE 1            │
                   │  Context → Plan →       │
                   │  Build → Validate       │
                   └────────────┬────────────┘
                                │ PASS
                                ▼
                   ┌─────────────────────────┐
                   │      PHASE 2            │
                   │  Context → Plan →       │◀──── REWORK (if failed)
                   │  Build → Validate       │
                   └────────────┬────────────┘
                                │ PASS
                                ▼
                              . . .
                                │
                                ▼
                   ┌─────────────────────────┐
                   │      PHASE N            │
                   │  Context → Plan →       │
                   │  Build → Validate       │
                   └────────────┬────────────┘
                                │ PASS
                                ▼
                   ┌─────────────────────────┐
                   │  FINAL SYNTHESIS         │
                   │  Orchestrator composes   │
                   │  results, reports to     │
                   │  user                    │
                   └─────────────────────────┘
```

### 2.3 Engineering Team Internal Flow

```
  Engineering Lead receives phase-spec.md
       │
       ├──────────────────────┐
       │                      │
       ▼                      ▼
  ┌──────────┐         ┌──────────┐
  │ Backend  │         │ Frontend │       (parallel when possible)
  │ Dev      │         │ Dev      │
  └────┬─────┘         └────┬─────┘
       │                    │
       └────────┬───────────┘
                ▼
       ┌──────────────┐
       │ Playwright   │                   (after impl, needs code)
       │ Tester       │
       └──────┬───────┘
              │
              ▼
       ┌──────────────┐
       │ Code         │                   (final gate, reviews all)
       │ Reviewer     │
       └──────┬───────┘
              │
              ▼
       Engineering Lead composes report
```

---

## 3. Phase Pipeline (Detailed)

### 3.1 Step 1: Context Loading

**Agent:** Context Loader (standalone)  
**Trigger:** Orchestrator invokes before every phase  
**Duration:** Fast — read-only, no writes beyond its output

| Aspect | Detail |
|--------|--------|
| **Inputs** | Phase manifest, previous phase outputs, codebase state |
| **Process** | 1. Read `phases/manifest.md` for task scope and phase list<br>2. Read previous phase outputs from `phases/phase-{N-1}/`<br>3. Scan relevant source files (`src/`, `tests/`, `specs/`)<br>4. Identify changed files since last phase<br>5. Compile structured context document |
| **Output** | `phases/phase-{N}/context.md` — structured context document |
| **Gate** | Orchestrator reviews context for completeness |

The context document follows a fixed template:

```markdown
# Phase {N} Context

## Task Summary
{from manifest}

## Previous Phase Output
{summary of phase N-1 deliverables}

## Current Codebase State
- Files changed: {list}
- Open issues from prior phases: {list}
- Dependencies: {relevant packages, APIs}

## Relevant Specs
- {links to applicable specs}

## Phase Scope
{what this phase should accomplish}
```

### 3.2 Step 2: Planning

**Team:** Planning (Lead → Architect + Spec Writer)  
**Trigger:** Orchestrator delegates with context.md from Step 1  

| Aspect | Detail |
|--------|--------|
| **Inputs** | `phases/phase-{N}/context.md`, existing specs |
| **Process** | 1. Architect designs the phase approach (component changes, API contracts)<br>2. Spec Writer produces detailed spec with acceptance criteria<br>3. Planning Lead reviews and composes |
| **Output** | `phases/phase-{N}/plan.md` — architecture decisions<br>`phases/phase-{N}/spec.md` — detailed specification with acceptance criteria |
| **Gate** | Orchestrator reviews plan. Checks: scope creep, feasibility, spec completeness |

### 3.3 Step 3: Engineering

**Team:** Engineering (Lead → Backend Dev + Frontend Dev + Playwright Tester + Code Reviewer)  
**Trigger:** Orchestrator delegates with plan.md + spec.md from Step 2  

| Aspect | Detail |
|--------|--------|
| **Inputs** | `phases/phase-{N}/plan.md`, `phases/phase-{N}/spec.md` |
| **Process** | 1. Backend Dev + Frontend Dev implement (parallel when independent)<br>2. Playwright Tester writes & runs E2E tests against impl<br>3. Code Reviewer reviews all output (quality gate)<br>4. Engineering Lead composes report |
| **Output** | Source code in `src/`, tests in `tests/`<br>`phases/phase-{N}/build-report.md` — what was built, files changed, deviations |
| **Gate** | Code Reviewer must approve. Engineering Lead must confirm all spec items addressed. |

**Sequencing within Engineering:**
- Backend Dev and Frontend Dev run **in parallel** when the plan allows (e.g., independent components). When there's a dependency (e.g., Frontend needs API from Backend), Backend runs first.
- Playwright Tester runs **after** Backend and Frontend are done — needs working code to test against.
- Code Reviewer runs **last** — reviews all code from the other three.
- If Code Reviewer finds issues, Engineering Lead can re-delegate to the originating worker.

### 3.4 Step 4: Validation

**Team:** Validation (Lead → QA Engineer + Security Reviewer)  
**Trigger:** Orchestrator delegates with build-report.md + spec.md + code paths  

| Aspect | Detail |
|--------|--------|
| **Inputs** | `phases/phase-{N}/build-report.md`, `phases/phase-{N}/spec.md`, changed file paths |
| **Process** | 1. QA Engineer verifies functional correctness against spec<br>2. Security Reviewer audits for vulnerabilities<br>3. Validation Lead synthesizes |
| **Output** | `phases/phase-{N}/validation-report.md` — test results, security findings, go/no-go |
| **Gate** | Orchestrator reads validation report. Decides: PASS (next phase), REWORK (re-run failed step), or ABORT (escalate to user) |

### 3.5 Phase Gate (Orchestrator Decision)

After Step 4, the orchestrator evaluates:

| Condition | Action |
|-----------|--------|
| All tests pass, no security issues, spec fulfilled | **PASS** — proceed to Phase N+1 (or complete if last phase) |
| Minor issues found | **REWORK** — re-delegate to the relevant step (usually Engineering) with fix instructions |
| Fundamental design problem | **REWORK** — re-delegate to Planning with feedback |
| Unrecoverable issue or scope change needed | **ABORT** — report to user, ask for direction |
| Max rework attempts exceeded (3 per phase) | **ABORT** — report accumulated issues to user |

---

## 4. New Agents

### 4.1 Context Loader

```yaml
# Standalone agent — not part of any team
# Invoked directly by orchestrator before each phase
```

| Property | Value | Rationale |
|----------|-------|-----------|
| **Model** | `sonnet:high` | Read-only analysis doesn't need opus. Sonnet is faster and cheaper. |
| **Tools** | `read`, `bash` | Needs to read files and run `find`/`grep`/`git` commands. No `write`/`edit`. |
| **Domain (read)** | `**/*` | Must read everything — source, specs, phases, tests |
| **Domain (write)** | `phases/**/context.md`, `.pi/expertise/**` | Can ONLY write its context output and its own expertise. |
| **Skills** | `mental-model`, `active-listener`, `output-contract` | Structured output is critical. |
| **Expertise** | `./agents/context-loader-expertise.md` | Accumulates patterns about what context is most useful. |

**System prompt key behaviors:**
- Produce a consistent, structured context document every time
- Summarize — don't dump entire files into context (token efficiency)
- Highlight changes since last phase, not the entire codebase
- Flag risks or blockers discovered during context gathering
- Be fast — this is a speed-sensitive step

**Agent definition file:** `agents/context-loader.md`

### 4.2 Playwright Tester

```yaml
# Member of Engineering team
# Runs after Backend Dev and Frontend Dev, before Code Reviewer
```

| Property | Value | Rationale |
|----------|-------|-----------|
| **Model** | `sonnet:high` | E2E test writing is pattern-based; sonnet handles it well. Saves cost. |
| **Tools** | `read`, `write`, `edit`, `bash` | Must write test files and run Playwright CLI |
| **Domain (read)** | `**/*` | Needs to read source code, specs, and existing tests |
| **Domain (write)** | `tests/e2e/**`, `playwright.config.*`, `.pi/expertise/**` | E2E test files only, plus Playwright config |
| **Skills** | `mental-model`, `active-listener`, `output-contract`, `lessons-learned` | Standard worker skills |
| **Expertise** | `./engineering/playwright-tester-expertise.md` | Accumulates patterns about test selectors, flaky tests, setup/teardown |

**System prompt key behaviors:**
- Write deterministic, non-flaky E2E tests
- Use data-testid selectors (not CSS classes or text content)
- Always run tests after writing them — report pass/fail with details
- Cover the happy path first, then edge cases per the spec
- Report which spec acceptance criteria each test covers
- Follow Playwright best practices: auto-waiting, web-first assertions

**Agent definition file:** `agents/engineering/playwright-tester.md`

### 4.3 Code Reviewer

```yaml
# Member of Engineering team
# Runs LAST — after all other engineering work is complete
# Quality gate: must approve before phase proceeds to validation
```

| Property | Value | Rationale |
|----------|-------|-----------|
| **Model** | `opus:xhigh` | Code review requires deep reasoning, pattern recognition, and nuance. Worth the cost. |
| **Tools** | `read`, `bash` | Read-only for code. Can run linters/tests but cannot modify code. |
| **Domain (read)** | `**/*` | Must read all source, tests, specs |
| **Domain (write)** | `phases/**/review.md`, `.pi/expertise/**` | Writes review document only. Cannot fix code — reports to lead. |
| **Skills** | `mental-model`, `active-listener`, `self-validation`, `lessons-learned` | Self-validation important for thoroughness |
| **Expertise** | `./engineering/code-reviewer-expertise.md` | Accumulates patterns about common issues, project conventions |

**System prompt key behaviors:**
- Review for: correctness, security, performance, readability, spec compliance
- Produce a structured review with severity levels (critical, major, minor, nit)
- Critical/major issues block the phase — must be addressed before proceeding
- Minor/nit issues are noted but don't block
- Check that tests actually cover the spec's acceptance criteria
- Verify error handling, edge cases, and type safety
- Cannot modify code — writes review to `phases/phase-{N}/review.md`
- If issues found, Engineering Lead re-delegates to originating worker with review feedback

**Agent definition file:** `agents/engineering/code-reviewer.md`

**Key design choice — Code Reviewer in Engineering vs. Validation:**

| Option | Pros | Cons |
|--------|------|------|
| **In Engineering (chosen)** | Tight feedback loop with devs. Engineering Lead can immediately re-delegate fixes. Catches issues before code leaves the team. | Self-review concern (same team reviews its own code). |
| **In Validation** | Independent review. Clear separation of concerns. | Slower feedback loop — issues cross team boundaries. Validation Lead manages code review AND QA, which is a wide scope. |
| **Standalone** | Maximally independent. | No team to leverage; orchestrator must manage directly. |

**Mitigation for self-review concern:** The Code Reviewer is **read-only** — it cannot modify code, only report issues. It's structurally independent within the team (different domain.write). The Validation team still provides an additional independent check.

---

## 5. Updated Team Structure

### 5.1 New config.yaml

```yaml
# Multi-Team Agentic Coding Configuration

orchestrator:
  system_prompt: ./agents/orchestrator.md

# Standalone agents — invoked directly by orchestrator, not part of any team
standalone_agents:
  context-loader:
    name: Context Loader
    system_prompt: ./agents/context-loader.md

teams:
  planning:
    color: "#4A90D9"
    lead:
      name: Planning Lead
      system_prompt: ./agents/planning/lead.md
    members:
      - name: Architect
        system_prompt: ./agents/planning/architect.md
      - name: Spec Writer
        system_prompt: ./agents/planning/spec-writer.md

  engineering:
    color: "#50C878"
    lead:
      name: Engineering Lead
      system_prompt: ./agents/engineering/lead.md
    members:
      - name: Backend Dev
        system_prompt: ./agents/engineering/backend-dev.md
      - name: Frontend Dev
        system_prompt: ./agents/engineering/frontend-dev.md
      - name: Playwright Tester
        system_prompt: ./agents/engineering/playwright-tester.md
      - name: Code Reviewer
        system_prompt: ./agents/engineering/code-reviewer.md

  validation:
    color: "#E8633E"
    lead:
      name: Validation Lead
      system_prompt: ./agents/validation/lead.md
    members:
      - name: QA Engineer
        system_prompt: ./agents/validation/qa-engineer.md
      - name: Security Reviewer
        system_prompt: ./agents/validation/security-reviewer.md

paths:
  agents: ./agents
  sessions: ./sessions
  logs: ./logs
  phases: ./phases
```

### 5.2 What Changed vs. Current Config

| Change | Before | After |
|--------|--------|-------|
| **Standalone agents section** | Did not exist | New `standalone_agents` section for Context Loader |
| **Engineering members** | 2 (Backend, Frontend) | 4 (Backend, Frontend, Playwright Tester, Code Reviewer) |
| **Planning team** | Unchanged | Unchanged |
| **Validation team** | Unchanged | Unchanged (but now runs per-phase) |
| **Paths** | 3 paths | 4 paths (added `phases`) |

### 5.3 Implementation Note: `standalone_agents`

The current pi multi-team framework may not support a `standalone_agents` config key natively. Two implementation paths:

**Option A: Native `standalone_agents` support (preferred)**
- Add `standalone_agents` as a first-class config concept
- Orchestrator can delegate to standalone agents the same way it delegates to team leads
- Standalone agents have no lead — the orchestrator delegates to them directly
- Requires a pi framework change

**Option B: Single-member team (fallback)**
- If `standalone_agents` isn't feasible, create a `context` team with just a "Context Lead" who IS the context loader (a lead with read/bash tools instead of delegate)
- Breaks the "leads never execute" rule but is pragmatic
- Less clean but works within current framework

**Option C: Orchestrator inline capability (simplest fallback)**
- Give the orchestrator itself `read` and `bash` tools for context loading
- Orchestrator runs context loading inline before delegating to planning
- No new agent needed
- Risk: bloats the orchestrator's role and token usage

**Recommendation:** Start with Option B (single-member team) for immediate compatibility. Migrate to Option A when the framework supports it. Option C is a last resort.

### 5.4 Full Agent Roster

| Agent | Team | Model | Tools | Write Domain |
|-------|------|-------|-------|-------------|
| **Orchestrator** | — | `opus:xhigh` | `delegate` | `.pi/expertise/**` |
| **Context Loader** | standalone | `sonnet:high` | `read`, `bash` | `phases/**/context.md`, `.pi/expertise/**` |
| **Planning Lead** | planning | `opus:xhigh` | `delegate` | `.pi/expertise/**` |
| **Architect** | planning | `opus:xhigh` | `read`, `write`, `edit`, `bash` | `specs/**`, `.pi/expertise/**` |
| **Spec Writer** | planning | `opus:xhigh` | `read`, `write`, `edit`, `bash` | `specs/**`, `.pi/expertise/**` |
| **Engineering Lead** | engineering | `opus:xhigh` | `delegate` | `.pi/expertise/**` |
| **Backend Dev** | engineering | `opus:xhigh` | `read`, `write`, `edit`, `bash` | `src/backend/**`, `tests/backend/**`, `.pi/expertise/**` |
| **Frontend Dev** | engineering | `opus:xhigh` | `read`, `write`, `edit`, `bash` | `src/frontend/**`, `tests/frontend/**`, `.pi/expertise/**` |
| **Playwright Tester** | engineering | `sonnet:high` | `read`, `write`, `edit`, `bash` | `tests/e2e/**`, `playwright.config.*`, `.pi/expertise/**` |
| **Code Reviewer** | engineering | `opus:xhigh` | `read`, `bash` | `phases/**/review.md`, `.pi/expertise/**` |
| **Validation Lead** | validation | `opus:xhigh` | `delegate` | `.pi/expertise/**` |
| **QA Engineer** | validation | `sonnet:high` | `read`, `bash` | `tests/**`, `.pi/expertise/**` |
| **Security Reviewer** | validation | `opus:xhigh` | `read`, `bash` | `.pi/expertise/**` |

**Model cost optimization:** Context Loader, Playwright Tester, and QA Engineer use `sonnet:high` (pattern-based work). Code Reviewer uses `opus:xhigh` (deep reasoning required). All leads use `opus:xhigh` (coordination requires understanding).

---

## 6. Phase Connectivity & Artifacts

### 6.1 Directory Structure

```
phases/
├── manifest.md                    # Master task breakdown: all phases listed
├── phase-1/
│   ├── context.md                 # Step 1 output: Context Loader
│   ├── plan.md                    # Step 2 output: Architect (via Planning)
│   ├── spec.md                    # Step 2 output: Spec Writer (via Planning)
│   ├── build-report.md            # Step 3 output: Engineering Lead summary
│   ├── review.md                  # Step 3 output: Code Reviewer findings
│   ├── validation-report.md       # Step 4 output: Validation Lead summary
│   └── gate-decision.md           # Orchestrator's pass/rework/abort decision
├── phase-2/
│   ├── context.md
│   ├── plan.md
│   ├── spec.md
│   ├── build-report.md
│   ├── review.md
│   ├── validation-report.md
│   └── gate-decision.md
└── phase-N/
    └── ...
```

### 6.2 Phase Manifest (`phases/manifest.md`)

Created by the orchestrator at the start of a task, before Phase 1 begins.

```markdown
# Task: {task title}
Created: {date}

## Phases

### Phase 1: {title}
- Scope: {what this phase delivers}
- Status: complete | in-progress | pending
- Dependencies: none

### Phase 2: {title}
- Scope: {what this phase delivers}
- Status: pending
- Dependencies: Phase 1

### Phase N: {title}
- Scope: {what this phase delivers}
- Status: pending
- Dependencies: Phase N-1
```

### 6.3 Phase Handoff Mechanism

**How Phase N output feeds into Phase N+1:**

```
Phase N completes
    │
    ├─ Orchestrator writes phases/phase-{N}/gate-decision.md (PASS)
    │
    ├─ Orchestrator updates phases/manifest.md (Phase N → complete)
    │
    ├─ Orchestrator delegates to Context Loader for Phase N+1
    │     │
    │     └─ Context Loader reads:
    │           - phases/manifest.md (overall progress)
    │           - phases/phase-{N}/build-report.md (what was built)
    │           - phases/phase-{N}/review.md (outstanding issues)
    │           - phases/phase-{N}/validation-report.md (test results)
    │           - Current source code (post-phase-N state)
    │
    │     └─ Context Loader writes:
    │           - phases/phase-{N+1}/context.md
    │
    └─ Orchestrator delegates to Planning with context.md
          (Phase N+1 begins)
```

### 6.4 Gate Decision Document (`gate-decision.md`)

```markdown
# Phase {N} Gate Decision

## Decision: PASS | REWORK | ABORT

## Summary
{one paragraph}

## Checklist
- [ ] All spec acceptance criteria met
- [ ] Code review passed (no critical/major issues)
- [ ] Tests passing
- [ ] No security vulnerabilities
- [ ] No scope creep beyond phase boundaries

## Issues Carried Forward
{any minor issues deferred to next phase}

## Next Phase Instructions
{specific guidance for Phase N+1, if applicable}
```

### 6.5 What the Orchestrator Checks Before Advancing

1. **Validation report** — all tests pass, no critical security findings
2. **Code review** — no critical or major issues (minor/nits are OK)
3. **Spec compliance** — build report confirms all acceptance criteria addressed
4. **Rework count** — hasn't exceeded max retries (3) for this phase
5. **Scope integrity** — phase didn't introduce work that belongs to future phases

### 6.6 Write Domain Considerations for `phases/`

The `phases/` directory is a shared artifact space. Write access must be carefully scoped:

| Agent | Can write to `phases/` | What specifically |
|-------|----------------------|-------------------|
| Context Loader | Yes | `phases/phase-{N}/context.md` only |
| Architect | No — writes to `specs/` | Plans go through `specs/`, orchestrator references them. **Alternative: expand Architect domain to include `phases/**/plan.md`** |
| Spec Writer | No — writes to `specs/` | Same as above for `phases/**/spec.md` |
| Code Reviewer | Yes | `phases/phase-{N}/review.md` only |
| Engineering Lead | No — delegates only | Build report is composed by lead (written to `.pi/expertise/` or passed verbally to orchestrator) |
| Validation Lead | No — delegates only | Validation report composed by lead |
| Orchestrator | Yes (needs new write permission) | `phases/**/gate-decision.md`, `phases/manifest.md` |

**Key trade-off: Who writes Planning's phase artifacts?**

| Option | Approach | Pros | Cons |
|--------|----------|------|------|
| **A: Expand Planning domain (recommended)** | Give Architect write access to `phases/**/plan.md` and Spec Writer to `phases/**/spec.md` | Clean — artifacts land directly in phase dir. | Widens existing domain scope. |
| **B: Write to specs/, symlink** | Planning writes to `specs/phase-{N}-plan.md`, orchestrator references it | No domain changes needed. | Artifacts scattered across two dirs. |
| **C: Orchestrator copies** | Planning writes to `specs/`, orchestrator (with write access to `phases/`) copies to phase dir | Orchestrator controls artifact placement. | Orchestrator does file management (bloat). |

**Recommendation:** Option A — expand Architect's domain to `specs/**` + `phases/**/plan.md`, Spec Writer to `specs/**` + `phases/**/spec.md`. Minimal change, clean output.

Similarly, Engineering Lead and Validation Lead need a mechanism to write their reports. Since leads can only write to `.pi/expertise/**`, either:
- The orchestrator composes build/validation reports from lead responses (no file needed — the orchestrator captures the text)
- Or expand lead write permissions to `phases/**/build-report.md` and `phases/**/validation-report.md` respectively

**Recommendation:** Leads report back verbally (via delegation response). The orchestrator writes `build-report.md`, `validation-report.md`, and `gate-decision.md` to `phases/`. This requires expanding the orchestrator's write domain to include `phases/**`.

---

## 7. Open Questions & Trade-offs

### 7.1 Open Questions

| # | Question | Impact | Recommended Resolution |
|---|----------|--------|----------------------|
| 1 | Does pi support `standalone_agents` in config? | Determines Context Loader implementation | Check pi docs; fallback to single-member team |
| 2 | Can the orchestrator's write domain be expanded to `phases/**`? | Needed for manifest + gate decisions | Should be safe — orchestrator is trusted |
| 3 | Should Planning agents write directly to `phases/`? | Affects domain config | Yes — expand their write domain |
| 4 | How does the orchestrator decompose a task into phases? | Scope of this design | Orchestrator uses judgment; could also delegate to Planning Lead for phase breakdown |
| 5 | Max phases per task? | Prevents runaway costs | Suggest hard cap of 5 phases; orchestrator asks user to re-scope if more needed |
| 6 | Should Playwright Tester have dev server management capability? | E2E tests need a running app | Give `bash` tool access for `npm run dev` / process management |

### 7.2 Key Trade-offs

#### Trade-off 1: Per-Phase Validation vs. Final-Only Validation

| Aspect | Per-Phase (chosen) | Final-Only |
|--------|-------------------|------------|
| **Issue detection** | Early — caught in the phase that created them | Late — accumulated issues at the end |
| **Cost** | Higher — validation runs N times | Lower — validation runs once |
| **Rework cost** | Lower — fix issues in small scope | Higher — fixing late-discovered issues is harder |
| **Speed** | Slower per phase | Faster overall if no issues |

**Decision:** Per-phase validation. The cost of late-discovered issues far exceeds the cost of incremental validation. The orchestrator CAN skip validation for trivial phases (e.g., documentation-only changes).

#### Trade-off 2: Code Reviewer Model (Opus vs. Sonnet)

| Aspect | Opus (chosen) | Sonnet |
|--------|--------------|--------|
| **Quality** | Deep analysis, catches subtle bugs | Pattern-matching, misses nuance |
| **Cost** | ~5x more expensive | Cheaper |
| **Speed** | Slower | Faster |

**Decision:** Code review is a quality gate — the last defense before validation. Using a cheaper model here creates a false sense of security. Opus is worth the cost.

#### Trade-off 3: Playwright Tester in Engineering vs. Validation

| Aspect | Engineering (chosen) | Validation |
|--------|---------------------|------------|
| **Feedback loop** | Tight — tests written alongside code | Loose — tests written after code ships from engineering |
| **E2E as part of build** | Yes — tests are a deliverable of the phase | No — tests are a check on the deliverable |
| **Validation independence** | Validation team doesn't write tests | Validation team writes AND runs tests |

**Decision:** E2E tests are a build artifact, not just a validation step. The Playwright Tester writes tests as part of the engineering process. The QA Engineer in validation can still run existing tests and write additional regression tests.

#### Trade-off 4: Phase Granularity (Orchestrator Decides vs. Planning Decides)

| Aspect | Orchestrator decides (recommended) | Planning team decides |
|--------|-----------------------------------|----------------------|
| **Speed** | Faster — no delegation needed for decomposition | Slower — extra delegation round |
| **Quality** | Orchestrator may miss technical nuances | Architect has deeper technical judgment |
| **Control** | Orchestrator stays in control of workflow | Planning influences workflow shape |

**Decision:** Orchestrator decomposes the task into phases (it already understands the full user request). For complex tasks, the orchestrator CAN delegate phase decomposition to the Planning Lead as a pre-step. This is a judgment call, not a rule.

#### Trade-off 5: Context Loader Necessity

| Aspect | Dedicated Context Loader (chosen) | Orchestrator gathers context | No explicit context step |
|--------|-----------------------------------|-----------------------------|-----------------------|
| **Token efficiency** | Context Loader summarizes; orchestrator gets compact doc | Orchestrator reads everything; high token usage | Each team loads its own context; duplicated work |
| **Consistency** | Same structured format every time | Depends on orchestrator's attention | Inconsistent across teams |
| **Speed** | Extra agent invocation | No extra step | No extra step |
| **Cost** | Sonnet:high is cheap | Opus:xhigh is expensive for context gathering | Distributed cost across opus agents |

**Decision:** Dedicated Context Loader. The token efficiency argument is strongest — having a sonnet agent summarize context is far cheaper than having every opus agent independently read the same files.

### 7.3 Migration Path

1. **Phase 1 (immediate):** Add Playwright Tester and Code Reviewer to Engineering team. Update `config.yaml` and create agent definitions. This works with the existing single-pass workflow.
2. **Phase 2 (short-term):** Implement Context Loader (using single-member team fallback if `standalone_agents` isn't supported). Add `phases/` directory and manifest structure.
3. **Phase 3 (medium-term):** Update orchestrator system prompt to use phase-based workflow. Update `plan-build-validate.md` prompt template to reference the new flow.
4. **Phase 4 (optional):** Add native `standalone_agents` support to pi framework if not already available.

### 7.4 Cost Estimation

Assuming a 3-phase task with the new pipeline:

| Step | Model | Invocations per phase | Cost weight |
|------|-------|--------------------|-------------|
| Context Loader | sonnet:high | 1 | Low |
| Planning Lead | opus:xhigh | 1 | High |
| Architect | opus:xhigh | 1 | High |
| Spec Writer | opus:xhigh | 1 | High |
| Engineering Lead | opus:xhigh | 1-4 | High |
| Backend Dev | opus:xhigh | 1 | High |
| Frontend Dev | opus:xhigh | 1 | High |
| Playwright Tester | sonnet:high | 1 | Low |
| Code Reviewer | opus:xhigh | 1 | High |
| Validation Lead | opus:xhigh | 1 | High |
| QA Engineer | sonnet:high | 1 | Low |
| Security Reviewer | opus:xhigh | 1 | High |

**Per phase:** ~12 agent invocations (8 opus, 3 sonnet, 1 lead re-delegation avg)  
**Per 3-phase task:** ~36 agent invocations  
**Cost concern:** This is significantly more expensive than the current single-pass workflow. The trade-off is quality and reliability. To mitigate: the orchestrator should aggressively minimize phases (1 phase for small tasks).

---

## Appendix A: Files Requiring Changes

| File | Change Type | Owner |
|------|-------------|-------|
| `config.yaml` | Modify — add standalone_agents, new engineering members, phases path | Requires framework knowledge |
| `agents/context-loader.md` | Create — new agent definition | Planning team |
| `agents/engineering/playwright-tester.md` | Create — new agent definition | Planning team |
| `agents/engineering/code-reviewer.md` | Create — new agent definition | Planning team |
| `agents/engineering/lead.md` | Modify — add delegation rules for new members | Planning/Engineering team |
| `agents/orchestrator.md` | Modify — add phase-based workflow instructions | Planning team |
| `prompts/plan-build-validate.md` | Modify or replace — update to phase-based flow | Planning team |
| `agents/planning/architect.md` | Modify — expand write domain to `phases/**/plan.md` | Planning team |
| `agents/planning/spec-writer.md` | Modify — expand write domain to `phases/**/spec.md` | Planning team |

## Appendix B: Glossary

| Term | Definition |
|------|-----------|
| **Phase** | A self-contained increment of work within a larger task. Has its own plan-build-validate cycle. |
| **Phase Pipeline** | The 4-step process within each phase: Context → Plan → Build → Validate. |
| **Phase Gate** | Orchestrator decision point at the end of each phase: PASS, REWORK, or ABORT. |
| **Phase Manifest** | Master document listing all phases for a task, with status tracking. |
| **Standalone Agent** | An agent invoked directly by the orchestrator, not part of any team hierarchy. |
| **Quality Gate** | A step that must pass before work proceeds. Code Reviewer is the engineering quality gate. |
| **Rework** | Re-running a step within a phase due to issues found at the gate. |
