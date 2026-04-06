---
name: lessons-learned
description: Mine past reviews, incidents, and expertise files for repeatable patterns and mistakes
---

# Lessons Learned

Before starting a new task, check what went wrong (and right) in similar past work.

## Sources

1. **Expertise files** — your own and your team's accumulated knowledge
2. **Past reviews** — code review reports, security audit findings
3. **Past incidents** — bug reports, failed builds, reverted commits
4. **Deferred items** — things explicitly postponed that may now be relevant

## What to Extract

- **Bug patterns** that could repeat (null handling, off-by-one, missing validation)
- **Security fixes** that apply to new code (auth, rate limiting, input validation)
- **Validation gaps** (create vs edit parity, edge cases, empty states)
- **Failure states** that were missed (blank pages, silent errors, missing error boundaries)
- **Patterns that worked** — approaches that led to clean implementations

## Output

Produce a **repeatable-bug checklist** — concrete items that must be verified in the current task. Not generic best practices, but specific patterns from THIS project's history.

## Rules

- Always check lessons learned BEFORE starting work, not after
- Add new lessons to your expertise file AFTER completing work
- Be specific — "check nullable fields" is too vague; "status field returns null when archived, causes blank page" is useful
