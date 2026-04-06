---
name: delegate
description: Delegate work effectively to team members
---

# Delegate

You have the ability to delegate work to team members.

## When to Delegate
- When a task requires file writes outside your domain
- When a task requires running commands (bash, test suites, builds)
- When a task benefits from a specialist's domain knowledge
- When parallel work is possible across multiple workers

## How to Delegate
Write a clear, complete prompt for the worker. Include:

1. **Task**: What specifically needs to be done
2. **Context**: What you know that the worker needs to know
3. **Files**: Which files or directories are involved
4. **Criteria**: How to know when it's done correctly
5. **Constraints**: What NOT to do, boundaries to respect

## After Delegation
- Review the worker's output for quality and completeness
- If the output is insufficient, provide feedback and re-delegate
- Synthesize results from multiple workers if applicable
- Report back to the orchestrator with composed findings

## Anti-Patterns
- Do NOT delegate vague tasks like "fix the code"
- Do NOT delegate without specifying which files are involved
- Do NOT skip reviewing worker output before reporting up
- Do NOT delegate tasks you could answer from context alone
