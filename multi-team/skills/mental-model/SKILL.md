---
name: mental-model
description: Maintain a personal expertise file that persists across sessions
---

# Mental Model Maintenance

You maintain a personal expertise file that persists across sessions.

## When to Read
- Always load your expertise file at the start of every session
- Read it before starting any task to recall past context

## When to Update
- After completing a significant task
- When you discover a pattern worth remembering
- When you make a mistake worth avoiding next time
- When you learn something about the codebase architecture

## How to Structure Entries
Use this format for each entry:

### [Topic] — [Date or Session]
- **Context**: What was happening
- **Insight**: What you learned
- **Action**: How this should affect future work

## Rules
- Keep entries high-level, not line-by-line details
- Remove outdated entries when the codebase changes
- Consolidate duplicate insights into single entries
- Never exceed max_lines from your front matter
- Track: patterns, risks, decisions, architecture, gotchas
- You own this file. No one else edits it. Maintain it well.
