---
name: output-contract
description: Every code-producing task must declare its output contract — files, exports, and build status
---

# Output Contract

Every task that produces code must declare what it outputs so downstream tasks can verify their inputs.

## Required Fields

1. **Files** — list of created/modified files with full paths
2. **Exports** — public interfaces, method names, types, module registrations
3. **Build status** — does it compile? (`pass` / `fail` / `not checked`)
4. **Module status** — registered in the module system? imports updated?

## Why

- The next task in the chain validates that its inputs exist
- Circular or missing dependencies are caught early
- Rollback is possible per-task (each output contract = one atomic commit)

## Rules

- Every code task MUST end with an output contract block
- Review/test/security tasks are exempt
- If an export from task N doesn't match an import in task N+1, flag it immediately
- Output contracts go into the deliverables summary, not buried in prose
