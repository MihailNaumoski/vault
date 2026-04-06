Read all agent `.md` files from `multi-team/agents/` (including subdirectories) and display a comprehensive agent inventory.

## Steps

1. Find all `.md` files in `multi-team/agents/` that are NOT expertise files (skip `*-expertise.md`)
2. For each agent file, parse the YAML front matter to extract: name, model, skills, tools, domain.write, expertise path
3. Check the corresponding expertise file: get line count and word count
4. Sort agents by team, then by tier (lead first, workers after)

## Display Format

```
📋 Agent Inventory

ORCHESTRATOR
  Orchestrator (opus:xhigh)
    Skills: 5 | Tools: delegate | Expertise: 0 lines
    Path: multi-team/agents/orchestrator.md

PLANNING
  Planning Lead (opus:xhigh) — lead
    Skills: 5 | Tools: delegate | Expertise: 12 lines
    Path: multi-team/agents/planning/lead.md
  Architect (opus:xhigh) — worker
    Skills: 4 | Tools: read,write,edit,bash | Domain: specs/**
    Expertise: 45 lines | Path: multi-team/agents/planning/architect.md
  Spec Writer (sonnet:high) — worker
    Skills: 4 | Tools: read,write,edit,bash | Domain: specs/**
    Expertise: 23 lines | Path: multi-team/agents/planning/spec-writer.md

ENGINEERING
  ...

VALIDATION
  ...

Total: X agents (1 orchestrator, 3 leads, 6 workers)
```

Derive all information from the actual files — don't hardcode values.
