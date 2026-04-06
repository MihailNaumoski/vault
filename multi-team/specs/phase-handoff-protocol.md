# Phase Handoff Protocol

**Author:** Spec Writer  
**Date:** 2026-04-06  
**Status:** Implementable  
**Input:** `specs/phase-workflow-architecture.md`, `agents/engineering/lead.md`, `agents/orchestrator.md`  
**Purpose:** Precise operational rules for phase lifecycle, artifact formats, handoff contracts, and agent delegation within the phase-based workflow.

---

## Table of Contents

1. [Phase Lifecycle](#1-phase-lifecycle)
2. [Artifact Specification](#2-artifact-specification)
3. [Handoff Contracts](#3-handoff-contracts)
4. [Orchestrator Phase Protocol](#4-orchestrator-phase-protocol)
5. [Engineering Lead Updated Delegation Rules](#5-engineering-lead-updated-delegation-rules)
6. [Edge Cases](#6-edge-cases)

---

## 1. Phase Lifecycle

### 1.1 What Is a Phase

A **phase** is a self-contained unit of work within a larger task. Each phase has its own context-gather → plan → build → validate cycle. Phases are sequential: Phase N cannot start until Phase N-1 has passed its gate.

A phase is NOT a milestone or a deployment unit — it is a workflow envelope that keeps scope manageable, context fresh, and validation incremental.

### 1.2 Phase Limits

| Constraint | Value | Rationale |
|------------|-------|-----------|
| **Max rework attempts per phase** | **3** | After 3 reworks, accumulated context drift and mounting issues signal a design problem — abort and ask the user |
| **Max phases per task** | **5** | Beyond 5 phases, the task is too large to manage as a single unit. Orchestrator should ask user to re-scope or split the task. |
| **Min phases per task** | **1** | A single-phase task is valid and common for small changes |

### 1.3 Phase Start Conditions (Preconditions)

A phase may start ONLY when ALL of the following are true:

1. **Previous phase complete:** `phases/phase-{N-1}/gate-decision.md` exists and has `Decision: PASS` (unless this is Phase 1).
2. **Manifest exists:** `phases/manifest.md` has been written by the orchestrator and includes this phase in its phase list.
3. **Context not stale:** If Phase N-1 was completed more than one session ago, the orchestrator must re-invoke Context Loader even if a `context.md` already exists from a prior attempt. A context document more than one session old is considered stale.
4. **Rework count not exceeded:** The rework count for this phase (tracked in `gate-decision.md`) is less than 3.

### 1.4 Phase End Conditions (Postconditions)

A phase is complete — with a gate decision — when ALL of the following artifacts exist:

| Artifact | Required to Gate |
|----------|-----------------|
| `phases/phase-{N}/context.md` | Yes |
| `phases/phase-{N}/plan.md` | Yes |
| `phases/phase-{N}/spec.md` | Yes |
| `phases/phase-{N}/build-report.md` | Yes |
| `phases/phase-{N}/review.md` | Yes |
| `phases/phase-{N}/validation-report.md` | Yes |
| `phases/phase-{N}/gate-decision.md` | Written BY the orchestrator AFTER reviewing the above |

A gate decision cannot be written if any of the first six artifacts are missing. If an artifact is missing, the orchestrator must determine which step failed and re-delegate that step.

### 1.5 Gate Decisions

The orchestrator makes one of three decisions after reviewing all phase artifacts:

| Decision | Condition | Action |
|----------|-----------|--------|
| **PASS** | All tests pass, no CRITICAL/MAJOR review findings, all acceptance criteria met, no unresolved security issues | Write `gate-decision.md` with PASS. Update `manifest.md`. Proceed to Phase N+1 (or final synthesis if last phase). |
| **REWORK** | Fixable issues found. Rework count < 3. | Write `gate-decision.md` with REWORK. Specify which step to re-run and what to fix. Re-delegate to the appropriate step. Increment rework counter. |
| **ABORT** | Unrecoverable issue, scope change required, or rework count = 3. | Write `gate-decision.md` with ABORT. Report to user with accumulated findings. Ask for direction. |

### 1.6 Rework Tracking

The `gate-decision.md` tracks rework cycles:

```
Rework Count: {n} / 3
```

The orchestrator increments this counter each time it writes a REWORK decision for the same phase. On the 4th gate evaluation (rework count = 3 attempts exhausted), the decision MUST be ABORT regardless of the severity of remaining issues.

### 1.7 Phase Numbering

Phases are numbered starting from 1. The directory names are `phases/phase-1/`, `phases/phase-2/`, etc. (not zero-indexed).

---

## 2. Artifact Specification

All artifacts live in `phases/phase-{N}/`. Each template below is the **exact required structure**. Sections marked **REQUIRED** must be present. Sections marked **OPTIONAL** may be omitted if not applicable, but must be explicitly stated as "N/A" rather than simply absent.

### 2.1 `context.md` — Context Loader Output

**Written by:** Context Loader  
**When:** Step 1 (before Planning)  
**Purpose:** Gives Planning and Engineering the information needed to do their work without reading the entire codebase themselves.

```markdown
# Phase {N} Context

**Generated:** {ISO date, e.g. 2026-04-06}
**Phase:** {N} of {total from manifest}
**Task:** {task title from manifest}

---

## 1. Task Summary                                          [REQUIRED]

{1–3 sentences from the manifest. What is the overall task? 
Use the manifest's language, not creative paraphrase.}

## 2. Phase Scope                                          [REQUIRED]

{What this specific phase must deliver. Be concrete: name components, 
endpoints, features, or files in scope.}

## 3. Previous Phase Output                               [REQUIRED]

{If Phase 1: "This is Phase 1 — no prior output."}
{If Phase N > 1:}

### 3a. What Was Built
{Bullet list from phase-{N-1}/build-report.md. One line per deliverable.}

### 3b. Outstanding Issues
{Issues from review.md or validation-report.md not yet resolved.
"None" if prior phase was clean.}

### 3c. Deferred Items
{Items from gate-decision.md explicitly deferred to this phase.
"None" if no deferrals.}

## 4. Current Codebase State                              [REQUIRED]

### 4a. Changed Files (Since Last Phase)
{List of files changed since phase N-1 completed.
Format: `path/to/file` — one-line description of what changed.
If Phase 1: recently modified files relevant to the task scope.}

### 4b. Relevant Source Structure
{Directory/file tree limited to what's directly relevant.
Maximum 30 lines. Annotate key files with a short comment.}

### 4c. Dependencies and Integrations                    [OPTIONAL]
{Packages, APIs, services, or internal modules this phase will touch.
"N/A" if no notable dependencies.}

## 5. Relevant Specs                                      [REQUIRED]

{List applicable spec files with their paths.
Format: `specs/filename.md` — what it covers.
"No prior specs — Planning will produce them." if none exist.}

## 6. Risk Flags                                          [REQUIRED]

{Risks discovered during context gathering.
Format: [SEVERITY] Title: Description. Recommended action: ...
"None identified." if clean.
Severity: CRITICAL | HIGH | MEDIUM | LOW}

## 7. Context Confidence                                  [REQUIRED]

{HIGH | MEDIUM | LOW}
{If not HIGH: explain what information is missing and why.}
```

**Length constraint:** ≤ 300 lines. If the context document exceeds 300 lines, the Context Loader is summarizing insufficiently — it must compress or trim.

---

### 2.2 `plan.md` — Architecture Plan

**Written by:** Architect (via Planning team)  
**When:** Step 2 (Planning)  
**Purpose:** Architecture decisions for this phase. How to build it — component structure, API contracts, data model changes, integration approach.

```markdown
# Phase {N} Plan

**Author:** Architect  
**Date:** {ISO date}  
**Phase:** {N}  

---

## 1. Approach Summary                                    [REQUIRED]

{2–4 sentences: What architectural approach will be used for this phase?
What are the key decisions?}

## 2. Component Changes                                   [REQUIRED]

{List of components, modules, or services being created or modified.

Format for each:
### {Component Name}
- **Action:** create | modify | delete
- **Location:** `path/to/file-or-directory`
- **Purpose:** {what this component does or will do}
- **Changes:** {specific changes if modifying existing}
}

## 3. API Contracts                                       [OPTIONAL]

{Required if any APIs are being created or changed.
"N/A" if no API changes.

Format for each endpoint:
### {METHOD} /path/to/endpoint
- **Purpose:** {what it does}
- **Request body:** {schema or "none"}
- **Response (success):** {schema, status code}
- **Response (error):** {error cases and status codes}
- **Authentication:** {required | not required | {method}}
}

## 4. Data Model Changes                                  [OPTIONAL]

{Required if database schema is changing.
"N/A" if no schema changes.
Include: new tables (with DDL), modified columns, new indexes, migrations needed.}

## 5. Integration Points                                  [OPTIONAL]

{External services, internal APIs, or shared modules this phase touches.
"N/A" if fully self-contained.}

## 6. Implementation Sequence                            [REQUIRED]

{Ordered list of what to build in what order.
Mark parallelizable items explicitly.

Example:
1. [PARALLEL] Backend: User model + migration
1. [PARALLEL] Frontend: Login form component (mocked API)
2. [SEQUENTIAL — after 1] Backend: Auth endpoints
3. [SEQUENTIAL — after 2] Frontend: Connect to real API
4. [SEQUENTIAL — after 3] E2E tests
5. [SEQUENTIAL — after 4] Code review
}

## 7. Trade-offs and Decisions                           [REQUIRED]

{Key trade-offs considered. At least 1 entry required.

Format:
### {Decision}
- **Options considered:** {list}
- **Decision:** {what was chosen}
- **Rationale:** {why}
- **Risk:** {what could go wrong with this choice}
}

## 8. Out of Scope                                        [REQUIRED]

{Explicit list of things NOT being done in this phase that might seem relevant.
"None" if scope is clear.}
```

---

### 2.3 `spec.md` — Detailed Specification

**Written by:** Spec Writer (via Planning team)  
**When:** Step 2 (Planning), after `plan.md` is ready  
**Purpose:** Precise, testable requirements. Engineering implements to this spec. Validation checks against it.

```markdown
# Phase {N} Specification

**Author:** Spec Writer  
**Date:** {ISO date}  
**Phase:** {N}  
**Status:** Implementable

---

## 1. Overview                                            [REQUIRED]

{2–3 sentences: What does this phase deliver from the user's perspective?}

## 2. Acceptance Criteria                                 [REQUIRED]

{Every acceptance criterion must be:
- Prefixed with AC-{N} where N is the criterion number within this phase
- Testable: a test can be written that passes or fails against this criterion
- Specific: no ambiguous language ("handle errors" is not testable; "return 400 with error message X when Y" is)

Format:
### AC-1: {Short title}
**Description:** {What must be true for this to pass.}
**Input:** {What triggers this behavior.}
**Expected output / behavior:** {Precise result. Include status codes, field names, error messages.}
**Edge cases:** {List edge cases that must also be handled.}

Repeat for each criterion. Use sequential numbering: AC-1, AC-2, ..., AC-N.}

## 3. Non-Functional Requirements                        [OPTIONAL]

{Performance, security, accessibility, or other quality attributes.
"N/A" if none specified for this phase.

Format:
- **{Category}:** {Specific requirement, e.g. "API response time < 200ms at p99 under 100 concurrent users"}
}

## 4. Out of Scope                                        [REQUIRED]

{What this spec does NOT cover. Prevents scope creep.
"None — all relevant scope is specified above." if complete.}

## 5. Open Questions                                     [OPTIONAL]

{Unresolved ambiguities that Engineering should surface if encountered.
"None." if all questions are resolved.}
```

**Testability requirement:** Every AC must be directly verifiable by the Playwright Tester or QA Engineer. If an AC cannot be tested, it must be rewritten until it can.

---

### 2.4 `build-report.md` — Engineering Build Report

**Written by:** Orchestrator (from Engineering Lead's verbal report)  
**When:** After Step 3 (Engineering) completes  
**Purpose:** Record of what was actually built. Compared against `spec.md` by Code Reviewer and Validation team.

```markdown
# Phase {N} Build Report

**Author:** Orchestrator (from Engineering Lead report)  
**Date:** {ISO date}  
**Phase:** {N}  
**Rework Cycle:** {0 for first build, 1+ for rework builds}

---

## 1. Summary                                             [REQUIRED]

{2–3 sentences: What was built this phase? Any notable deviations?}

## 2. Files Changed                                       [REQUIRED]

{Complete list of files created, modified, or deleted.

Format:
| File | Action | Author | Description |
|------|--------|--------|-------------|
| `src/backend/user.service.ts` | created | Backend Dev | User CRUD service |
| `src/frontend/Login.tsx` | modified | Frontend Dev | Added OAuth flow |
| `tests/e2e/auth.spec.ts` | created | Playwright Tester | Auth E2E tests |
}

## 3. Acceptance Criteria Coverage                        [REQUIRED]

{For each AC in spec.md, confirm whether it was addressed.

Format:
| AC | Status | Notes |
|----|--------|-------|
| AC-1 | ✅ Complete | |
| AC-2 | ✅ Complete | |
| AC-3 | ⚠️ Partial | Missing validation for empty email |
| AC-4 | ❌ Not started | Requires external OAuth provider — flagged to orchestrator |
}

## 4. Deviations from Plan                               [REQUIRED]

{Any deviation from plan.md. "None." if built exactly as planned.

Format for each deviation:
- **Deviation:** {what changed}
- **Reason:** {why the plan was changed}
- **Impact:** {effect on other components or future phases}
}

## 5. Test Results                                       [REQUIRED]

{Summary of Playwright Tester's run results.

Format:
- E2E tests written: {n}
- E2E tests passing: {n}
- E2E tests failing: {n}
- Failing tests: {list with test name and error summary, or "None"}
}

## 6. Implementation Notes                               [OPTIONAL]

{Technical notes that future phases or the Code Reviewer should know.
"None." if nothing notable.}

## 7. Rework Fixes Applied (if applicable)               [OPTIONAL]

{If this is a rework build: what specific issues from the previous review were fixed.
"N/A — first build." if not a rework.

Format:
- **[CRITICAL] SQL injection in user endpoint** → Fixed by using parameterized queries in `user.repository.ts` line 47
}
```

---

### 2.5 `review.md` — Code Review

**Written by:** Code Reviewer  
**When:** During Step 3 (Engineering), after Playwright Tester completes  
**Purpose:** Quality gate. Documents all findings with severity. Determines whether phase may proceed to Validation.

```markdown
# Phase {N} Code Review

**Reviewer:** Code Reviewer  
**Date:** {ISO date}  
**Phase:** {N}  
**Rework Cycle:** {0 for first review, 1+ for subsequent}

---

## Decision: APPROVE | REWORK | BLOCK                    [REQUIRED]

> {One sentence explaining the decision.}

## Summary                                               [REQUIRED]

| Severity | Count |
|----------|-------|
| CRITICAL | {n} |
| MAJOR    | {n} |
| MINOR    | {n} |
| NIT      | {n} |
| **Total** | {n} |

**Blocking findings:** {n} (CRITICAL + MAJOR count)

---

## Findings                                              [REQUIRED]

{One entry per finding. "No findings." if APPROVE with nothing to note.}

### [{SEVERITY}] {Short Title} — `path/to/file:{line}`

{2–4 sentences: describe the problem, quote the relevant code, explain the risk, state the correct behavior.}

**Required fix:** {Concrete instruction. Name the file, function, and change needed.}
{Use "Suggested fix:" for MINOR/NIT instead of "Required fix:"}

---

## Spec Compliance Audit                                 [REQUIRED]

| Acceptance Criterion | Status | Notes |
|---------------------|--------|-------|
| AC-1: {description} | ✅ Implemented | |
| AC-2: {description} | ❌ Missing | No implementation found |
| AC-3: {description} | ⚠️ Partial | Implemented but missing edge case |

## Test Coverage Assessment                              [REQUIRED]

{2–4 sentences: Do E2E tests cover the critical paths? Are acceptance criteria untested? Are tests testing the right things?}

## Linter / Type-Check Results                          [REQUIRED]

{Output of `npm run lint`, `tsc --noEmit`, `cargo check`, etc.
"No linter configured." or "Clean." or paste relevant error output.}

## Previous Rework Verification (if applicable)         [OPTIONAL]

{If Rework Cycle ≥ 1: for each issue from the previous review, confirm resolution.
"N/A — first review." if not a rework cycle.

Format:
- [CRITICAL] SQL injection → RESOLVED (parameterized queries in user.repository.ts)
- [MAJOR] Missing auth check → STILL PRESENT (user.controller.ts line 22)
}
```

**Severity definitions** (for reference in this document):

| Level | Meaning | Blocks gate? |
|-------|---------|--------------|
| CRITICAL | Data loss, security breach, system crash, or complete feature failure | **Yes** |
| MAJOR | Incorrect behavior, significant performance issue, missing spec requirement | **Yes** |
| MINOR | Code quality issue, convention deviation, suboptimal approach | No |
| NIT | Stylistic, minor naming, trivial inconsistency | No |

---

### 2.6 `validation-report.md` — Validation Team Report

**Written by:** Orchestrator (from Validation Lead's verbal report)  
**When:** After Step 4 (Validation) completes  
**Purpose:** Independent verification of functional correctness and security. Feeds directly into gate decision.

```markdown
# Phase {N} Validation Report

**Author:** Orchestrator (from Validation Lead report)  
**Date:** {ISO date}  
**Phase:** {N}  

---

## Overall Result: PASS | FAIL                           [REQUIRED]

> {One sentence explaining the result.}

---

## QA Engineer Findings                                  [REQUIRED]

### Functional Correctness

| Acceptance Criterion | Result | Notes |
|---------------------|--------|-------|
| AC-1: {description} | ✅ PASS | |
| AC-2: {description} | ❌ FAIL | {What failed and how} |
| AC-3: {description} | ⚠️ PARTIAL | {What works and what doesn't} |

### Test Execution Summary

{Tests run, pass count, fail count. Include specific failure details.}

### Edge Cases Tested

{List of edge cases checked beyond the acceptance criteria. "None beyond AC coverage." is acceptable.}

---

## Security Reviewer Findings                           [REQUIRED]

### Security Audit Result: PASS | FAIL | NO ISSUES

{If PASS with issues: list findings.
If NO ISSUES: "No security concerns identified."
"N/A — security reviewer determined this phase has no security-sensitive changes." is acceptable for trivial phases.}

| Finding | Severity | Description | Recommendation |
|---------|----------|-------------|----------------|
| {finding} | CRITICAL/HIGH/MEDIUM/LOW | {description} | {what to do} |

---

## Validation Blockers                                  [REQUIRED]

{List any findings that must be fixed before the gate can PASS.
"None — validation passed cleanly." if no blockers.}

## Deferred Observations                                [OPTIONAL]

{Non-blocking observations for future phases.
"None." if nothing to defer.}
```

---

### 2.7 `gate-decision.md` — Orchestrator Gate Decision

**Written by:** Orchestrator  
**When:** After reviewing all six previous artifacts  
**Purpose:** Official phase gate decision. The authoritative record of phase outcome, rework tracking, and next-phase instructions.

```markdown
# Phase {N} Gate Decision

**Author:** Orchestrator  
**Date:** {ISO date}  
**Phase:** {N}  
**Rework Count:** {n} / 3

---

## Decision: PASS | REWORK | ABORT                      [REQUIRED]

> {One paragraph: What was the decision and why? Name the specific factors that drove it.}

---

## Gate Checklist                                        [REQUIRED]

| Criterion | Status | Source |
|-----------|--------|--------|
| All spec acceptance criteria met | ✅ / ❌ / ⚠️ | build-report.md §3 |
| No CRITICAL/MAJOR review findings | ✅ / ❌ | review.md §Summary |
| E2E tests passing | ✅ / ❌ | build-report.md §5 |
| Validation: functional correctness | ✅ / ❌ | validation-report.md |
| Validation: no critical security issues | ✅ / ❌ | validation-report.md |
| Rework count within limit (< 3) | ✅ / ❌ | This document |
| No scope creep beyond phase boundaries | ✅ / ❌ | review.md §Spec Compliance |

## Issues Carried Forward                               [OPTIONAL]

{Minor/nit issues from review.md that are NOT blocking but should be noted.
"None." if gate is clean.}

## Rework Instructions (if Decision = REWORK)           [CONDITIONAL]

{Required only if Decision = REWORK. Omit if PASS or ABORT.

- **Re-run step:** {Context Loading | Planning | Engineering | Validation}
- **Reason:** {Specific issues to fix, referencing review.md or validation-report.md finding numbers}
- **Instructions for step:** {Precise direction for the re-delegated step}
- **Do NOT re-run:** {Steps that were clean and don't need repeating}
}

## Abort Reason (if Decision = ABORT)                   [CONDITIONAL]

{Required only if Decision = ABORT. Omit if PASS or REWORK.

- **Reason for abort:** {Why this phase cannot continue}
- **Accumulated issues:** {Summary of all unresolved blockers across all rework cycles}
- **User guidance:** {What the orchestrator will tell the user / what options exist}
}

## Next Phase Instructions (if Decision = PASS)         [CONDITIONAL]

{Required only if Decision = PASS AND there is a next phase.
Guidance for the Context Loader and Planning team for Phase N+1.

- **Carry forward:** {Issues deferred, dependencies to be aware of}
- **Scope for Phase N+1:** {What the next phase covers, per manifest}
- **Notes for Planning:** {Any architectural guidance from this phase's learnings}
}
```

---

## 3. Handoff Contracts

A handoff contract defines: what the sender must provide, what the receiver expects, and what happens when the handoff is incomplete.

### 3.1 Handoff 1: Context → Planning

**Trigger:** Context Loader writes `phases/phase-{N}/context.md` and reports completion to orchestrator.  
**Sender:** Orchestrator (passing context to Planning Lead)  
**Receiver:** Planning Lead → Architect + Spec Writer

#### What the Sender Must Provide

The orchestrator's delegation message to Planning Lead MUST include:

1. **File reference:** `phases/phase-{N}/context.md` — explicitly tell Planning to read this file first
2. **Phase number and scope:** State which phase this is and what it covers
3. **Output targets:** Instruct Planning to produce `phases/phase-{N}/plan.md` and `phases/phase-{N}/spec.md`
4. **Any orchestrator-level flags:** If context.md had HIGH risk flags, surface them explicitly
5. **Context confidence level:** Tell Planning if the context was HIGH/MEDIUM/LOW confidence

**Required delegation format:**
```
Phase {N} Planning:

Read phases/phase-{N}/context.md first — this contains the codebase state and phase scope.
Context confidence: {HIGH|MEDIUM|LOW}. {If MEDIUM/LOW: describe the gap.}

Produce:
- phases/phase-{N}/plan.md — architecture decisions
- phases/phase-{N}/spec.md — acceptance criteria

Risk flags from context (if any): {list or "none"}
```

#### What the Receiver Expects

Planning Lead MUST verify before delegating to Architect:
- `phases/phase-{N}/context.md` exists and has all 7 required sections
- Section 2 (Phase Scope) is specific enough to plan against
- Section 7 (Context Confidence) is present

If context.md is missing sections or confidence is LOW: Planning Lead must report back to orchestrator before proceeding. Do not plan against incomplete context.

#### Incomplete Handoff Recovery

| Missing Element | Action |
|----------------|--------|
| `context.md` doesn't exist | Orchestrator re-invokes Context Loader |
| `context.md` missing required sections | Orchestrator notes the gaps and re-invokes Context Loader with specific instructions |
| Context confidence = LOW with critical missing info | Orchestrator re-invokes Context Loader after addressing the stated gaps (e.g., if git was unavailable, fix that first) |
| Phase scope is ambiguous | Planning Lead reports back: orchestrator resolves the ambiguity before re-delegating |

---

### 3.2 Handoff 2: Planning → Engineering

**Trigger:** Planning Lead delivers `plan.md` and `spec.md` to orchestrator.  
**Sender:** Orchestrator (passing plan + spec to Engineering Lead)  
**Receiver:** Engineering Lead → Backend Dev + Frontend Dev + Playwright Tester + Code Reviewer

#### What the Sender Must Provide

The orchestrator's delegation message to Engineering Lead MUST include:

1. **File references:** `phases/phase-{N}/plan.md` AND `phases/phase-{N}/spec.md` — both
2. **Phase number:** Explicit
3. **Implementation sequence:** The plan's §6 (Implementation Sequence) — tell Engineering the order
4. **Parallelization guidance:** Which workers can run in parallel (per the plan)
5. **Output expectation:** Build report, review, all source code changes
6. **Any orchestrator constraints:** Budget limits, specific technologies to avoid, etc.

**Required delegation format:**
```
Phase {N} Engineering:

Read:
- phases/phase-{N}/plan.md — architecture decisions and implementation sequence
- phases/phase-{N}/spec.md — acceptance criteria you must implement

The implementation sequence (per plan §6):
{paste the sequence from plan.md §6}

Expected outputs:
- Source code changes per the plan
- phases/phase-{N}/build-report.md (compose from your team's work)
- phases/phase-{N}/review.md (from Code Reviewer)
- E2E tests in tests/e2e/

{Any special constraints or notes}
```

#### What the Receiver Expects

Engineering Lead MUST verify before delegating to workers:
- `phases/phase-{N}/plan.md` exists with all required sections (§1–8)
- `phases/phase-{N}/spec.md` exists with at least one AC
- Each AC in spec.md is testable (has concrete expected behavior)
- The implementation sequence (plan §6) is clear enough to create worker assignments

If spec.md contains untestable ACs: Engineering Lead must report back to orchestrator. Engineering should not build to a spec that cannot be verified.

#### Incomplete Handoff Recovery

| Missing Element | Action |
|----------------|--------|
| `plan.md` doesn't exist | Orchestrator re-delegates to Planning Lead |
| `spec.md` doesn't exist | Orchestrator re-delegates to Planning Lead |
| AC has no expected behavior | Engineering Lead reports back; orchestrator re-delegates spec to Spec Writer for clarification |
| Implementation sequence missing | Engineering Lead reports back; orchestrator re-delegates to Architect for §6 |
| Plan has no API contracts for an API-heavy phase | Engineering Lead reports back before building; orchestrator resolves with Architect |

---

### 3.3 Handoff 3: Engineering → Validation

**Trigger:** Engineering Lead delivers build report and review to orchestrator.  
**Sender:** Orchestrator (passing artifacts to Validation Lead)  
**Receiver:** Validation Lead → QA Engineer + Security Reviewer

#### What the Sender Must Provide

The orchestrator's delegation message to Validation Lead MUST include:

1. **File references:** `phases/phase-{N}/spec.md`, `phases/phase-{N}/build-report.md`, `phases/phase-{N}/review.md`
2. **Changed file list:** Explicitly list which source files changed (from build-report.md §2)
3. **Code review status:** Whether Code Reviewer issued APPROVE, REWORK, or BLOCK — and the finding count
4. **Test results from Engineering:** E2E test pass/fail summary (from build-report.md §5)
5. **Explicit validation scope:** What Validation must check (functional + security)

**Note:** Validation should NOT proceed if review.md has a BLOCK decision (no rework cycle occurred). The orchestrator must resolve the BLOCK first via Engineering rework before sending to Validation.

**Required delegation format:**
```
Phase {N} Validation:

Read:
- phases/phase-{N}/spec.md — what was supposed to be built
- phases/phase-{N}/build-report.md — what was built, files changed
- phases/phase-{N}/review.md — code review findings

Code review decision: {APPROVE | REWORK_COMPLETE}
Review summary: {CRITICAL: n, MAJOR: n, MINOR: n, NIT: n}

Files changed this phase:
{list from build-report.md §2}

E2E test results from Engineering: {pass/fail summary}

Validate:
1. Functional correctness against spec.md acceptance criteria
2. Security audit on changed files
3. Any edge cases not covered by Engineering's tests
```

#### What the Receiver Expects

Validation Lead MUST verify before delegating:
- `spec.md` has acceptance criteria with expected behaviors (needed for QA)
- `build-report.md` §2 (files changed) is complete — QA and Security need to know what to look at
- `review.md` decision is APPROVE or REWORK_COMPLETE (not BLOCK)

If review.md shows BLOCK: Validation Lead must report back to orchestrator without proceeding. Validation does not override a BLOCK.

#### Incomplete Handoff Recovery

| Missing Element | Action |
|----------------|--------|
| `build-report.md` doesn't exist | Orchestrator re-delegates to Engineering Lead to produce it |
| `review.md` shows BLOCK | Orchestrator routes back to Engineering for rework; Validation waits |
| `spec.md` ACs have no expected behavior | Validation Lead reports back; cannot validate without expected behavior |
| Files-changed list is vague ("modified some files") | Validation Lead asks orchestrator to get specifics from Engineering Lead |
| No E2E tests were written | Validation Lead notes this and QA Engineer increases coverage of manual verification |

---

### 3.4 Handoff 4: Validation → Gate

**Trigger:** Validation Lead delivers validation report to orchestrator.  
**Sender:** Validation Lead (via orchestrator's delegate response)  
**Receiver:** Orchestrator (makes the gate decision)

#### What the Sender Must Provide

The Validation Lead's response to the orchestrator MUST include:

1. **Overall result:** PASS or FAIL (clear, unambiguous)
2. **Per-AC functional results:** Table of AC-N → PASS/FAIL/PARTIAL
3. **Security findings:** List with severity, or "No issues"
4. **Blockers:** Explicit list of what must be fixed before PASS (or "None")
5. **QA Engineer test details:** Which tests were run, what failed
6. **Security Reviewer scope:** What was audited

#### What the Receiver (Orchestrator) Does With It

1. Orchestrator reads the validation report
2. Orchestrator reads `review.md` (already available — from Engineering step)
3. Orchestrator checks the gate checklist (see `gate-decision.md` template §Gate Checklist)
4. Orchestrator makes the PASS/REWORK/ABORT decision
5. Orchestrator writes `phases/phase-{N}/gate-decision.md`
6. Orchestrator updates `phases/manifest.md` (marks phase status)

#### Incomplete Handoff Recovery

| Missing Element | Action |
|----------------|--------|
| Validation report is missing per-AC results | Orchestrator asks Validation Lead to complete the report before gate |
| Validation result is "PARTIAL" without explanation | Orchestrator asks for specifics; cannot gate on ambiguity |
| Security reviewer did not audit | Orchestrator notes this in gate-decision.md; must decide if the phase scope was truly security-free |
| Validation Lead report contradicts QA test results | Orchestrator notes the discrepancy; may re-delegate QA to clarify |

---

## 4. Orchestrator Phase Protocol

### 4.1 Task Decomposition

The orchestrator decomposes a task into phases **before delegating to any team**. This is an orchestrator-level judgment call, not delegated to Planning.

**Decision criteria for phase count:**

| Task characteristic | Phase guidance |
|---------------------|---------------|
| Single-component change, < 3 acceptance criteria | 1 phase |
| Multi-component change with clear dependency order | 1 phase per dependency layer (e.g., data model → API → UI = 3 phases) |
| Feature with backend + frontend + integration tests | 1–2 phases |
| Full feature stack + migration + deployment config | 3–4 phases |
| Requires user input to validate direction mid-way | Split at the validation point |
| Estimated to require > 5 phases | Too large — ask user to re-scope before starting |

**Heuristic:** When in doubt, use fewer phases. One well-scoped phase is better than two poorly-scoped ones. Phases can be added if a gate decision reveals unexpected scope.

**Phase scope principles:**
- Each phase must be completable without feedback from the next phase
- A phase should not depend on code that another phase will write (except explicitly: "Phase 2 builds on Phase 1's API")
- Each phase should have a coherent, user-understandable deliverable

### 4.2 Writing `phases/manifest.md`

The orchestrator writes `phases/manifest.md` immediately after deciding on the phase breakdown — before Phase 1 begins. This is the only file the orchestrator writes before the first Context Loader invocation.

**Manifest format:**

```markdown
# Task Manifest

**Task:** {title}  
**Created:** {ISO date}  
**Total Phases:** {n}  
**Status:** in-progress  

---

## Phase 1: {Title}

- **Scope:** {What this phase delivers. 2–4 sentences.}
- **Status:** in-progress | complete | pending
- **Dependencies:** none
- **Gate decision:** [pending]

## Phase 2: {Title}

- **Scope:** {What this phase delivers.}
- **Status:** pending
- **Dependencies:** Phase 1 complete
- **Gate decision:** [pending]

## Phase {n}: {Title}

- **Scope:** {What this phase delivers.}
- **Status:** pending
- **Dependencies:** Phase {n-1} complete
- **Gate decision:** [pending]
```

**Manifest updates:** After each phase gate:
- Update the phase's `Status` from `in-progress` to `complete` (PASS) or note the abort
- Update `Gate decision` with the actual decision date and result
- Mark the next phase `in-progress`

### 4.3 Orchestrator Step Delegation

For each step within a phase, the orchestrator delegates in sequence:

**Step 1 — Context Loading:**
```
Delegate to: Context Loader (standalone) or Context team lead (fallback)
Input: phases/manifest.md, phase number, prior phase directory path
Wait for: phases/phase-{N}/context.md to be written
```

**Step 2 — Planning:**
```
Delegate to: Planning Lead
Input: phases/phase-{N}/context.md, phase number
Wait for: Planning Lead report confirming plan.md and spec.md are written
```

**Step 3 — Engineering:**
```
Delegate to: Engineering Lead
Input: phases/phase-{N}/plan.md, phases/phase-{N}/spec.md, phase number
Wait for: Engineering Lead report (build-report.md and review.md written)
```

**Step 4 — Validation:**
```
Delegate to: Validation Lead
Input: phases/phase-{N}/spec.md, phases/phase-{N}/build-report.md, phases/phase-{N}/review.md, changed file list
Wait for: Validation Lead report (validation-report.md written)
```

**After Step 4 — Gate:**
```
Orchestrator evaluates all 6 artifacts
Orchestrator writes phases/phase-{N}/gate-decision.md
Orchestrator updates phases/manifest.md
```

### 4.4 Gate Decision Logic

The orchestrator applies this logic in order:

```
1. Check rework count:
   IF rework_count >= 3 → ABORT (regardless of remaining issues)

2. Check validation report:
   IF validation-report.md has FAIL AND blockers listed → REWORK or ABORT (see step 4)
   IF validation-report.md has PASS → proceed to step 3

3. Check code review:
   IF review.md has CRITICAL or MAJOR findings → REWORK (re-run Engineering)
   (Note: if validation PASS but review has CRITICAL, review takes precedence — REWORK)

4. Check severity of remaining issues:
   IF issues are fixable in < 1 rework cycle → REWORK
   IF issues indicate fundamental design problem → consider ABORT
   IF issues require user input (scope change, external dependency) → ABORT

5. Check spec compliance:
   IF any AC in spec.md is not addressed (❌ in build-report.md or review.md) → REWORK

6. All checks pass → PASS
```

### 4.5 Rework Loop Mechanics

When the gate decision is REWORK:

1. **Identify the step to re-run.** Usually Engineering (code issues, missing ACs). Sometimes Planning (if Code Reviewer or Validation found a design flaw). Rarely Context Loading (only if context was materially wrong).

2. **Do NOT re-run steps that were clean.** If Context Loading and Planning were correct, don't re-invoke them. Send Engineering directly to the fix with the review findings as input.

3. **Pass the review findings explicitly.** When re-delegating Engineering, include the specific findings from `review.md` or `validation-report.md` that must be fixed.

4. **Engineering Lead does not need to re-run all workers.** If only Backend Dev's code had issues, re-delegate only to Backend Dev (and then Code Reviewer again). Frontend Dev and Playwright Tester don't re-run unless their code was also cited.

5. **Code Reviewer always re-runs.** Even if only one worker did rework, Code Reviewer must review the result. The review document (`review.md`) is overwritten with the new review (Rework Cycle incremented).

6. **Validation always re-runs after a rework.** The validation-report.md is overwritten with a new validation pass.

7. **Gate-decision.md is overwritten.** Each gate decision replaces the prior one. The rework count is the only carryover.

**Rework re-delegation format:**
```
Phase {N} Engineering Rework (Cycle {n}):

The prior gate decision was REWORK. Address the following issues:

From review.md:
- [CRITICAL] {finding title}: {description and required fix}
- [MAJOR] {finding title}: {description and required fix}

From validation-report.md:
- AC-{n}: FAIL — {what failed and expected behavior}

Workers to re-invoke:
- {Backend Dev | Frontend Dev} — fix the issues listed above
- Code Reviewer — re-review all changed files after fixes (update review.md, Rework Cycle {n})

Do NOT re-invoke:
- {workers whose code had no issues}

After rework is complete, report back for gate re-evaluation.
```

### 4.6 After Final Phase: Synthesis

When the last phase PASS gate is written:

1. Update `phases/manifest.md` — mark all phases complete, overall status = complete
2. Compile a final synthesis response to the user:
   - What was built (across all phases)
   - Notable decisions made (from plan.md files)
   - Any deferred items not addressed (from gate-decision.md "Issues Carried Forward" sections)
   - A clear statement of what the user can now do or verify

---

## 5. Engineering Lead Updated Delegation Rules

This section replaces the current delegation rules in `agents/engineering/lead.md`. The Engineering Lead now manages 4 workers instead of 2.

### 5.1 Worker Roster and Responsibilities

| Worker | Responsibility | When Invoked |
|--------|---------------|--------------|
| **Backend Dev** | APIs, business logic, data models, backend tests, database queries | When spec includes server-side changes |
| **Frontend Dev** | UI components, state management, routing, client-side logic, frontend tests | When spec includes client-side changes |
| **Playwright Tester** | E2E test writing and execution covering spec acceptance criteria | Always — after Backend Dev and Frontend Dev complete |
| **Code Reviewer** | Quality review of all code produced this phase; writes `review.md` | Always — after Playwright Tester completes |

### 5.2 Mandatory Sequencing

```
Phase Engineering Step:

[PARALLEL — may run simultaneously]
  Backend Dev (server-side implementation)
  Frontend Dev (client-side implementation) *

[SEQUENTIAL — after both above complete]
  Playwright Tester (E2E tests against the implementation)

[SEQUENTIAL — after Playwright Tester completes]
  Code Reviewer (reviews ALL code from this phase)

[FINAL — Engineering Lead composes report]
  Compile build-report.md from all worker outputs
  Pass to orchestrator
```

*Exception: if Frontend requires a Backend API to function, Backend must complete before Frontend starts (sequential).

### 5.3 Determining Parallel vs. Sequential (Backend + Frontend)

Run **in parallel** when:
- Frontend is building UI components with mocked/stubbed API responses
- Backend is building a service that frontend doesn't yet call
- The two workers' file scopes do not overlap

Run **sequentially (Backend first)** when:
- Frontend requires actual API responses to function correctly
- Frontend needs Backend's type definitions or interfaces
- The plan explicitly states a sequential dependency (see plan.md §6)

**How to decide:** Check `phases/phase-{N}/plan.md` §6 (Implementation Sequence). If it marks the Backend/Frontend pair as `[PARALLEL]`, run them in parallel. If it marks Backend as `[SEQUENTIAL]` before Frontend, run them sequentially.

### 5.4 Delegation Message Requirements

**To Backend Dev:**
```
Phase {N} Backend Implementation:

Read:
- phases/phase-{N}/plan.md — architecture and API contracts (§3)
- phases/phase-{N}/spec.md — acceptance criteria to implement

Implement: {specific backend components from plan.md §2}
Files to create/modify: {list from plan.md §2 and §3}

Verify by: running backend tests after implementation.
Report: what you built, which files changed, any deviations from the plan, test results.
```

**To Frontend Dev:**
```
Phase {N} Frontend Implementation:

Read:
- phases/phase-{N}/plan.md — architecture and component structure (§2)
- phases/phase-{N}/spec.md — acceptance criteria to implement
{If sequential: "Backend API is complete. Endpoints available: {list from Backend Dev's report}"}

Implement: {specific frontend components from plan.md §2}
Files to create/modify: {list from plan.md §2}

Verify by: running frontend unit tests and manually checking rendered components.
Report: what you built, which files changed, any deviations from the plan, test results.
```

**To Playwright Tester:**
```
Phase {N} E2E Testing:

Read:
- phases/phase-{N}/spec.md — acceptance criteria to cover (AC-1 through AC-{n})
- phases/phase-{N}/plan.md — implementation sequence and component list
- Backend Dev report: {files changed, test results}
- Frontend Dev report: {files changed, test results}

Write E2E tests covering all acceptance criteria.
Run tests immediately after writing each file.
Report: spec coverage table (AC-N → test name → PASS/FAIL), any implementation bugs found.
```

**To Code Reviewer:**
```
Phase {N} Code Review:

Read:
- phases/phase-{N}/spec.md — what was supposed to be built
- phases/phase-{N}/build-report.md (draft) — what was built, files changed
- All source files changed this phase: {list from worker reports}
- All E2E test files written by Playwright Tester: {list}

Review all files for: correctness, security, performance, readability, spec compliance.
Write your findings to: phases/phase-{N}/review.md

Report back: your decision (APPROVE | REWORK | BLOCK) and finding counts by severity.
```

### 5.5 Handling Code Reviewer Findings

After receiving the Code Reviewer's report:

**If Code Reviewer decision = APPROVE:**
- Compose the build-report.md from all worker outputs
- Report to orchestrator: engineering complete, review approved, ready for validation

**If Code Reviewer decision = REWORK:**
- Identify which worker(s) own the CRITICAL/MAJOR findings
- Re-delegate ONLY to the worker(s) who own the issues, with the review findings as context:
  ```
  Rework needed per code review. Address these specific findings:
  - [CRITICAL] {title}: {file:line} — {description} — Required fix: {instruction}
  - [MAJOR] {title}: {file:line} — {description} — Required fix: {instruction}
  ```
- After the worker's rework is complete, re-delegate to Code Reviewer (Rework Cycle increment)
- Do NOT re-delegate Playwright Tester unless its tests were cited in findings
- Repeat until APPROVE or until rework count would exceed 3 (escalate to orchestrator)

**If Code Reviewer decision = BLOCK:**
- Do NOT attempt rework within the Engineering step
- Report to orchestrator immediately: "Code Reviewer issued BLOCK. Reason: {summary}. This requires orchestrator-level decision (REWORK to Planning or ABORT)."
- BLOCK means the Engineering Lead cannot resolve the issue — it signals a design flaw or scope problem

**Engineering Lead rework escalation limit:** If Code Reviewer issues REWORK on 2 consecutive cycles within the same phase, Engineering Lead should escalate to orchestrator rather than attempting a 3rd rework. The orchestrator decides whether to continue or abort.

### 5.6 Build Report Composition

The Engineering Lead composes the `build-report.md` and reports it verbally to the orchestrator (who writes it to `phases/phase-{N}/build-report.md`).

The Engineering Lead's report to the orchestrator must include ALL sections of the build-report.md template:
1. Summary
2. Files Changed (from all worker reports combined)
3. Acceptance Criteria Coverage (checked against spec.md)
4. Deviations from Plan
5. Test Results (from Playwright Tester's coverage table)
6. Implementation Notes (if any)
7. Rework Fixes Applied (if applicable)

The Engineering Lead does NOT write to `phases/` directly. It delivers a structured verbal report; the orchestrator writes the file.

---

## 6. Edge Cases

### 6.1 Empty Phase (No Code Changes)

A phase may be documentation-only, configuration-only, or consist only of spec updates. In this case:

- Engineering step: Backend Dev and Frontend Dev may have nothing to do. Engineering Lead should report this to orchestrator before delegating unnecessarily.
- Playwright Tester: Skip if there's nothing to test (no behavior change). Engineering Lead notes "No E2E tests written — this phase has no behavior changes."
- Code Reviewer: Still reviews any files that changed (even docs or config can have issues)
- Validation: QA Engineer focuses on config correctness; Security Reviewer may skip if no code changed

**The orchestrator may skip Engineering and/or Validation for documentation-only phases.** This is an explicit orchestrator judgment call, noted in `gate-decision.md`.

### 6.2 Rework Loop Exhaustion

When a phase reaches Rework Count = 3 and the gate still cannot PASS:

1. Orchestrator writes `gate-decision.md` with `Decision: ABORT`
2. Orchestrator writes a summary of ALL unresolved issues across all 3 rework cycles
3. Orchestrator presents this to the user with options:
   - Re-scope the phase (narrow the acceptance criteria)
   - Provide additional context (maybe the architecture is wrong)
   - Abandon the task
4. The user's response determines whether a new `phases/manifest.md` is written

**The orchestrator must not silently continue past 3 reworks.** This limit exists precisely because 3 failed attempts signal that something structural is wrong — more reworks won't fix it.

### 6.3 Phase Failure Partway Through

If a step fails to produce its artifact (agent errors, tool failures, etc.):

- Orchestrator detects the missing artifact after the delegation returns
- Orchestrator re-delegates to that step with specific instructions about what was missing
- This does NOT count as a rework cycle (the rework counter only increments after a full gate evaluation)
- If a step fails 3 times to produce its artifact, orchestrator treats it as an ABORT condition

### 6.4 Max Phases Exceeded

If the task requires more than 5 phases:

1. Before starting Phase 6, orchestrator stops and reports to the user:
   - Phases 1–5 completed (list their deliverables)
   - Remaining scope that doesn't fit in the limit
   - Request: "Please start a new task for the remaining scope, or confirm you want to continue past the 5-phase limit."
2. The user can explicitly override the limit, in which case orchestrator documents the override in `phases/manifest.md`

### 6.5 Context Loader Cannot Find Prior Phase Output

If Context Loader is invoked for Phase N > 1 but `phases/phase-{N-1}/` doesn't have complete artifacts:

1. Context Loader sets Context Confidence = LOW
2. Context Loader lists what's missing in Risk Flags section
3. Context Loader still produces the context document with what it can find
4. Orchestrator sees the LOW confidence + risk flags
5. Orchestrator decides: is the missing prior context critical? If yes, investigate the prior phase artifacts before proceeding. If no, proceed with caution noted in the gate-decision for Phase N-1.

### 6.6 Code Reviewer and Playwright Tester Invocation on Phase 1

In Phase 1, there is no prior code to compare against. Both agents should:

- **Playwright Tester:** Test the code as written, not "what changed." In Phase 1, everything is new — write tests for all acceptance criteria.
- **Code Reviewer:** Review all code written in this phase. No prior baseline is needed — review the code as if reviewing a new pull request.

### 6.7 Engineering Lead Re-delegation Limit

If Engineering Lead finds itself on the 3rd rework cycle of delegating within a single phase's Engineering step (i.e., Code Reviewer has been run 3 times within the same Engineering step):

1. Engineering Lead does NOT start a 4th rework cycle
2. Engineering Lead escalates to orchestrator immediately with a summary of all 3 review cycles
3. Orchestrator decides: is this an Engineering-fixable problem or a Planning-fixable problem?
4. If Planning: orchestrator routes to Planning (counts as 1 rework at the phase level)
5. If unresolvable: orchestrator ABORTs

---

## Appendix: Artifact Existence Checklist

Use this checklist to confirm all artifacts are present before running a gate decision:

```
Phase {N} Pre-Gate Artifact Checklist:

[ ] phases/manifest.md — exists and Phase N is listed
[ ] phases/phase-{N}/context.md — all 7 sections present, confidence stated
[ ] phases/phase-{N}/plan.md — all required sections present
[ ] phases/phase-{N}/spec.md — at least 1 AC present, all ACs testable
[ ] phases/phase-{N}/build-report.md — files-changed list, AC coverage table, test results
[ ] phases/phase-{N}/review.md — decision stated, all findings have severity, spec compliance table
[ ] phases/phase-{N}/validation-report.md — overall result stated, per-AC table, security audit result

All 7 checked → proceed to gate decision
Any unchecked → identify which step must re-run and re-delegate before gate
```
