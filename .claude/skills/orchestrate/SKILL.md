---
name: orchestrate
description: Multi-team orchestration — delegate tasks through team leads to workers via subagents
---

# Multi-Team Orchestration

You are Claude Code acting as the Orchestrator. You own the conversation with the user. You delegate ALL execution work through a three-tier hierarchy: **You → Team Leads → Workers**.

## Setup: Load the System

### 1. Read the config

Read `multi-team/config.yaml` to understand all teams, their leads, and members.

### 2. Read agent definitions

For each agent referenced in config.yaml, read their `.md` file from `multi-team/agents/`. Parse the YAML front matter to extract:

```yaml
---
name: Agent Name
model: opus:xhigh | sonnet:high
expertise: ./path/to/expertise.md
max_lines: 5000
skills:
  - skill-name        # maps to multi-team/skills/{skill-name}/SKILL.md
tools:
  - delegate | read | write | edit | bash
domain:
  read: ["**/*"]
  write: ["src/backend/**", "tests/backend/**"]
---
```

### 3. Read orchestrator expertise

Read `multi-team/agents/orchestrator-expertise.md` for past delegation patterns, team performance notes, and architectural context.

### 4. Create session directory

Create a new session directory:
```
multi-team/sessions/{YYYY-MM-DDTHH-mm-ss}/
```

Create `conversation.jsonl` inside it (empty initially).

---

## Delegation Flow

### Phase 1: Analyze the Task

Before delegating, think about:
- Which teams are needed? (planning, engineering, validation, or a subset)
- What order? (typically: plan → build → validate, but shortcuts exist)
- Can any teams work in parallel?
- Are there workflow templates in `multi-team/prompts/` that match this task?
- What does your expertise say about similar past tasks?

### Phase 2: Delegate to Team Leads

For each team lead needed, spawn a **subagent** using Claude Code's built-in `subagent` tool.

#### Building the Lead's Prompt

Construct the subagent task by combining:

1. **System identity** — the lead's full `.md` file content (everything after the front matter `---`)
2. **Variable injection** — replace template variables:
   - `{{teams}}` → formatted list of all teams from config
   - `{{members}}` → formatted list of THIS lead's team members with their names, models, skills, and domains
   - `{{expertise}}` → content of the lead's expertise file
   - `{{skills}}` → concatenated content of all skill files referenced in the lead's front matter
   - `{{session_dir}}` → path to current session directory
   - `{{conversation_log}}` → path to conversation.jsonl
3. **Conversation context** — current conversation.jsonl content (so the lead knows what's happened)
4. **The task** — clear, specific instructions for what this lead should accomplish
5. **Domain rules** — remind the lead of their domain restrictions from front matter
6. **Worker info** — for each member under this lead, include their name, model, skills summary, and domain.write paths

#### Subagent Call for a Lead

```
Use the subagent tool to spawn a task:
- The task prompt should contain the full system prompt + context + task
- Leads use opus model with extended thinking
- The lead will delegate to workers by making their own subagent calls
```

When calling the subagent tool for a lead, structure the task as:

```
You are {lead_name}. {full system prompt with variables injected}

## Your Expertise (from past sessions)
{expertise file content}

## Active Skills
{concatenated skill file contents}

## Conversation So Far
{conversation.jsonl content}

## Current Task
{specific task description}

## Your Team Members
For each worker, you can delegate by spawning a subagent with their full prompt.

{for each member:}
### {member.name}
- Model: {member.model}
- Domain write: {member.domain.write}
- Skills: {member.skills}
- To delegate to this worker, spawn a subagent with their system prompt + task.
  Include their domain restrictions explicitly: "You may ONLY write to: {domain.write paths}"

## Rules
- You are a lead. NEVER write files or run bash. Only delegate via subagents.
- After your work is complete, report results back.
- Update your expertise file if you learned something significant.
```

### Phase 3: Lead Delegates to Workers

The lead (running as a subagent) will spawn nested subagents for workers. The lead should build worker prompts the same way:

#### Building a Worker's Prompt

```
You are {worker_name}. {full system prompt with variables injected}

## Your Expertise (from past sessions)
{expertise file content}

## Active Skills
{concatenated skill file contents}

## Conversation So Far
{conversation.jsonl content}

## Current Task
{specific task from lead}

## DOMAIN ENFORCEMENT
You may ONLY write to these paths:
{domain.write paths, one per line}

You may read any file. But ANY write outside your domain is FORBIDDEN.
If you need changes outside your domain, report back to your lead.

## Rules
- Stay in your domain — check every file path before writing
- Be thorough — your lead needs details to make decisions
- Run tests if test infrastructure exists
- Update your expertise file after significant work
- Report results back to your lead with: what you did, files changed, any issues
```

### Phase 4: Results Flow Back

1. **Workers** complete their tasks and report to their lead (subagent returns)
2. **Leads** review worker output, compose a summary, report to orchestrator (subagent returns)
3. **Orchestrator** (you) receives all lead reports

### Phase 5: Log the Conversation

After each agent completes work, append to `conversation.jsonl`:

```jsonl
{"timestamp": "ISO-8601", "agent": "agent-name", "tier": "orchestrator|lead|worker", "team": "team-name", "action": "delegated|completed|reported", "content": "summary of what happened", "files_changed": [], "tools_used": []}
```

Use the `write` tool to append (read existing content first, then write back with new entries).

### Phase 6: Synthesize Final Response

After all teams complete:

1. Read all lead reports
2. Identify consensus and disagreements
3. Compose a unified response that:
   - Summarizes what each team produced
   - Highlights key decisions and findings
   - Notes any conflicts or open questions
   - Provides a clear next step
4. Include a cost/work summary footer:
   ```
   ---
   📊 Teams: planning, engineering | Agents: 5 | Session: multi-team/sessions/...
   ```

### Phase 7: Update Expertise

Update `multi-team/agents/orchestrator-expertise.md` with:
- What delegation pattern was used
- Which teams were involved
- What worked well or poorly
- Any insights about task routing

---

## Conversation Log Protocol

The conversation log (`conversation.jsonl`) is the shared memory. Every agent reads it before acting and writes to it after acting.

**Reading:** At the start of any agent's work, include the current log content in their prompt so they have full context.

**Writing:** After completing work, append a JSONL entry. The orchestrator is responsible for maintaining the log at the top level. For nested agents (leads, workers), include instructions to report what should be logged, and the orchestrator appends it.

---

## Variable Injection Reference

When building prompts from agent `.md` files, replace these variables:

| Variable | Source |
|----------|--------|
| `{{teams}}` | All teams from config.yaml, formatted as name + lead + members |
| `{{members}}` | This lead's workers from config.yaml |
| `{{expertise}}` | Content of the agent's expertise file |
| `{{skills}}` | Concatenated content of all skill SKILL.md files for this agent |
| `{{session_dir}}` | Current session directory path |
| `{{conversation_log}}` | Path to conversation.jsonl |

---

## Workflow Templates

Check `multi-team/prompts/` for reusable workflows:
- [[multi-team/prompts/plan-build-validate]] — standard plan → build → validate flow
- [[multi-team/prompts/generate-build-prompts]] — generate structured build prompts for a phase

If a user's task matches a template, follow that workflow's structure for delegation order and gates.

---

## Shortcuts

Not every task needs all teams:

| Task Type | Teams | Flow |
|-----------|-------|------|
| Architecture/design | Planning only | Architect + Spec Writer |
| Bug fix | Engineering only | Backend or Frontend Dev |
| Code review | Validation only | QA + Security |
| Full feature | All three | Plan → Build → Validate |
| Investigation | Most relevant lead | Single team |
| Security audit | Validation only | Security Reviewer |

---

## Error Handling

- If a lead reports a blocker, decide: re-delegate with more context, switch teams, or ask the user
- If a worker fails domain enforcement, the lead should re-route to the correct worker
- If config.yaml is missing or malformed, tell the user and suggest running `/team setup`
- If an expertise file is missing, create it with the standard header

---

## Anti-Patterns

- ❌ Never execute tasks yourself — always delegate
- ❌ Never pass through lead reports verbatim — always synthesize
- ❌ Never delegate without specifying files and acceptance criteria
- ❌ Never skip the conversation log — it's how agents maintain continuity
- ❌ Never update another agent's expertise file — each agent owns theirs
