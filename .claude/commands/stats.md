Read all expertise files and session data to show system statistics.

## Steps

1. Find all `*-expertise.md` files in `multi-team/agents/` (including subdirectories)
2. For each file: count lines, count words, get last modified date via `stat`
3. Sort by line count descending
4. Find the latest session directory in `multi-team/sessions/`
5. If a `conversation.jsonl` exists in the latest session, count entries and show summary

## Display Format

```
📊 Multi-Team Statistics

EXPERTISE FILES (sorted by size)
┌──────────────────────────────────┬───────┬───────┬─────────────┬────────┐
│ File                             │ Lines │ Words │ Last Modified│ Budget │
├──────────────────────────────────┼───────┼───────┼─────────────┼────────┤
│ orchestrator-expertise.md        │   45  │  320  │ 2026-04-04  │ 0.5%   │
│ planning/lead-expertise.md       │   38  │  245  │ 2026-04-04  │ 0.8%   │
│ engineering/backend-dev-exp...   │   12  │   89  │ 2026-04-03  │ 0.2%   │
│ ...                              │       │       │             │        │
└──────────────────────────────────┴───────┴───────┴─────────────┴────────┘
Total: X lines across Y files

LATEST SESSION
  Directory: multi-team/sessions/2026-04-04T21-30-38/
  Entries: 12 conversation log entries
  Agents involved: Orchestrator, Planning Lead, Architect, ...

SESSION HISTORY
  Total sessions: N
  Most recent: {date}
```

Budget % = (current lines / max_lines from agent front matter) × 100.

Read `max_lines` from each agent's `.md` front matter to calculate budget usage.
