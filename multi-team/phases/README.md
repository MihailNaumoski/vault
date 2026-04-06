# Phases

This directory holds per-phase artifacts for the phase-based development workflow.

Each phase gets a subdirectory: `phase-1/`, `phase-2/`, etc.

Per-phase artifacts:
- `context.md` — Context Loader output
- `plan.md` — Architecture plan
- `spec.md` — Detailed specification
- `build-report.md` — What was built
- `review.md` — Code review findings
- `validation-report.md` — QA + security results
- `gate-decision.md` — Orchestrator pass/rework/abort decision

The `manifest.md` file (in this directory) tracks all phases for a task.
