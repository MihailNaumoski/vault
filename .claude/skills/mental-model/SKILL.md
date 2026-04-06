---
name: mental-model
description: Consolidate and manage expertise files across all agents — used by /dream
---

# Expertise Consolidation (Dream Mode)

Use this skill to maintain, consolidate, and prune all agent expertise files in the multi-team system.

## Overview

Every agent maintains a personal expertise file (`*-expertise.md`) that grows over time. Without maintenance, these files become bloated, duplicative, and stale. The `/dream` command triggers this consolidation process.

---

## Step 1: Inventory

Read all expertise files:
```
multi-team/agents/orchestrator-expertise.md
multi-team/agents/planning/lead-expertise.md
multi-team/agents/planning/architect-expertise.md
multi-team/agents/planning/spec-writer-expertise.md
multi-team/agents/engineering/lead-expertise.md
multi-team/agents/engineering/backend-dev-expertise.md
multi-team/agents/engineering/frontend-dev-expertise.md
multi-team/agents/validation/lead-expertise.md
multi-team/agents/validation/qa-engineer-expertise.md
multi-team/agents/validation/security-reviewer-expertise.md
```

For each file, record:
- Line count
- Word count
- Number of entries (sections with `###` headers)
- Last meaningful content (not just the template header)

Also read each agent's front matter to get `max_lines`.

---

## Step 2: Analyze Each File

For each expertise file with content beyond the template header:

### Check for Staleness
- Entries referencing files/APIs/patterns that no longer exist in the codebase
- Entries about resolved bugs or completed migrations
- Entries contradicted by newer entries in the same file

### Check for Duplication
- Multiple entries saying the same thing differently
- Entries that overlap with another agent's expertise (cross-agent duplication is OK if domain-specific, bad if identical)

### Check for Bloat
- Entries that are too verbose (line-by-line details instead of high-level patterns)
- Entries that state obvious things (e.g., "always run tests")
- Entries without actionable insights

### Check Line Budget
- Compare current line count to `max_lines` from front matter
- If over 80% of budget, aggressive consolidation is needed
- If under 30%, file is healthy

---

## Step 3: Consolidate

For each file that needs work:

1. **Remove** stale entries (reference check against codebase)
2. **Merge** duplicate entries — keep the most complete version
3. **Condense** verbose entries — extract the actionable pattern
4. **Reorder** — group by topic (architecture, patterns, risks, gotchas, decisions)
5. **Preserve** the file header (`# {Name} Expertise` + italics warning + comment block)

### Consolidation Rules
- Never remove an entry unless you've verified it's stale (check if referenced files/patterns still exist)
- When merging, keep the most specific version with the most context
- Keep date/session references when they add context
- Maintain the agent's voice — don't rewrite their insights, just tighten them

---

## Step 4: Report

After consolidation, produce a report:

```markdown
## Expertise Consolidation Report

| Agent | Before (lines) | After (lines) | Budget (max) | Entries Removed | Entries Merged | Status |
|-------|----------------|---------------|--------------|-----------------|----------------|--------|
| Orchestrator | 45 | 38 | 10000 | 2 | 1 | ✅ healthy |
| Planning Lead | 120 | 85 | 5000 | 5 | 3 | ✅ consolidated |
| ... | ... | ... | ... | ... | ... | ... |

### Changes Made
- **Orchestrator**: Removed 2 stale entries about old API routing. Merged 1 duplicate pattern.
- **Planning Lead**: Condensed 3 verbose architecture entries. Removed 2 entries about completed migration.
...

### Recommendations
- Backend Dev expertise is at 85% capacity — consider raising max_lines or archiving old entries
- Security Reviewer has no entries — this agent hasn't been used enough yet
```

---

## Agent Expertise Guidelines

These are the rules agents follow when updating their OWN expertise files during normal work. Include these in agent prompts via the [[multi-team/skills/mental-model/SKILL.md]] skill.

### When to Update
- After completing a significant task (not trivial fixes)
- When discovering a new pattern in the codebase
- When making a mistake worth avoiding next time
- When learning something about architecture or dependencies
- After a task that required unexpected workarounds

### How to Structure Entries

```markdown
### [Topic] — [Date or Context]
- **Context**: What was happening when this was learned
- **Insight**: The actual lesson or pattern
- **Action**: How this should affect future work
```

### What to Track
- **Patterns**: Recurring code patterns, naming conventions, structural choices
- **Risks**: Things that tend to break, fragile areas, implicit dependencies
- **Decisions**: Why something was done a certain way (architecture decision records)
- **Architecture**: Component boundaries, data flow, service relationships
- **Gotchas**: Non-obvious behaviors, edge cases, platform quirks

### Rules for Agents
- Keep entries high-level — not line-by-line code details
- Remove outdated entries when the codebase changes
- Consolidate duplicate insights into single entries
- Never exceed `max_lines` from your front matter
- You own this file — no one else edits it
- Format entries consistently so `/dream` can parse them
- If your file is getting large, self-consolidate before adding more
