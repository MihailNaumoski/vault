# Multi-Team Agentic Coding System

This vault contains a multi-team orchestration system for agentic coding.

## Quick Start

- `/team` — Start orchestration mode (delegates to team leads → workers)
- `/team <task>` — Start orchestration with a specific task immediately
- `/teams` — List all teams, leads, and members
- `/agents` — Show all agent definitions with model and expertise info
- `/stats` — Expertise file sizes and session costs
- `/dream` — Consolidate and prune all agent expertise files

## Architecture

- **Orchestrator → Team Leads → Workers** (three tiers)
- You talk ONLY to the orchestrator. It delegates everything.
- Leads think and coordinate. Workers execute within domain-locked directories.

## Key Files

- Team config: [[multi-team/config.yaml]]
- Agent definitions: `multi-team/agents/` (YAML front matter + system prompt)
- Agent expertise: `multi-team/agents/*-expertise.md` (agent-maintained — don't edit manually)
- Agent behavior skills: `multi-team/skills/` (injected into subagent prompts)
- Claude Code skills: `.claude/skills/` (how Claude Code runs the system)
- Session logs: `multi-team/sessions/`
- Workflow templates: `multi-team/prompts/`

## Rules

- Domain enforcement is always active (see `.claude/rules/multi-team.md`)
- Leads NEVER execute — only delegate via subagents
- Workers NEVER write outside their `domain.write` paths
- Expertise files with "do not edit manually" are agent-maintained
- Every agent reads the conversation log before responding

## Dashboard

- [[multi-team/DASHBOARD]] — Full team overview with wikilinks
