---
name: setup
description: Bootstrap the multi-team system from scratch or add to a new project
---

# Multi-Team Setup

Use this skill to bootstrap the full multi-team agentic coding system or add it to a new project.

---

## Full Bootstrap (New Vault)

### 1. Create Directory Structure

```bash
mkdir -p multi-team/agents/planning
mkdir -p multi-team/agents/engineering
mkdir -p multi-team/agents/validation
mkdir -p multi-team/skills/{mental-model,zero-micromanagement,conversational-response,active-listener,delegate,output-contract,self-validation,lessons-learned}
mkdir -p multi-team/sessions
mkdir -p multi-team/prompts
mkdir -p .claude/skills/{orchestrate,agent-creator,mental-model,setup}
mkdir -p .claude/commands
mkdir -p .claude/rules
```

### 2. Create Agent Behavior Skills

These go in `multi-team/skills/` and are injected into agent prompts. Each is a directory with a `SKILL.md` file.

Create these skill files (see existing files for content):
- `multi-team/skills/mental-model/SKILL.md` — how agents maintain expertise
- `multi-team/skills/zero-micromanagement/SKILL.md` — leads never execute
- `multi-team/skills/conversational-response/SKILL.md` — concise responses
- `multi-team/skills/active-listener/SKILL.md` — read conversation log first
- `multi-team/skills/delegate/SKILL.md` — how to delegate effectively
- `multi-team/skills/output-contract/SKILL.md` — declare outputs per task
- `multi-team/skills/self-validation/SKILL.md` — validate before presenting
- `multi-team/skills/lessons-learned/SKILL.md` — mine past work for patterns

### 3. Create Core Agents

Use the [[.claude/skills/agent-creator/SKILL.md]] templates to create:

**Orchestrator:**
- `multi-team/agents/orchestrator.md` — system prompt with YAML front matter
- `multi-team/agents/orchestrator-expertise.md` — empty expertise file

**Planning Team:**
- `multi-team/agents/planning/lead.md`
- `multi-team/agents/planning/lead-expertise.md`
- `multi-team/agents/planning/architect.md`
- `multi-team/agents/planning/architect-expertise.md`
- `multi-team/agents/planning/spec-writer.md`
- `multi-team/agents/planning/spec-writer-expertise.md`

**Engineering Team:**
- `multi-team/agents/engineering/lead.md`
- `multi-team/agents/engineering/lead-expertise.md`
- `multi-team/agents/engineering/backend-dev.md`
- `multi-team/agents/engineering/backend-dev-expertise.md`
- `multi-team/agents/engineering/frontend-dev.md`
- `multi-team/agents/engineering/frontend-dev-expertise.md`

**Validation Team:**
- `multi-team/agents/validation/lead.md`
- `multi-team/agents/validation/lead-expertise.md`
- `multi-team/agents/validation/qa-engineer.md`
- `multi-team/agents/validation/qa-engineer-expertise.md`
- `multi-team/agents/validation/security-reviewer.md`
- `multi-team/agents/validation/security-reviewer-expertise.md`

### 4. Create config.yaml

Write `multi-team/config.yaml` with the full team structure. See [[multi-team/config.yaml]] for the format.

### 5. Create Claude Code Plugin Files

- `CLAUDE.md` — short pointer file (always loaded)
- `.claude/skills/orchestrate/SKILL.md` — main orchestration brain
- `.claude/skills/agent-creator/SKILL.md` — create new agents
- `.claude/skills/mental-model/SKILL.md` — expertise consolidation
- `.claude/skills/setup/SKILL.md` — this file
- `.claude/commands/team.md` — `/team` command
- `.claude/commands/teams.md` — `/teams` command
- `.claude/commands/agents.md` — `/agents` command
- `.claude/commands/stats.md` — `/stats` command
- `.claude/commands/dream.md` — `/dream` command
- `.claude/rules/multi-team.md` — domain enforcement rules

### 6. Create Workflow Templates

- `multi-team/prompts/plan-build-validate.md` — standard three-phase workflow
- `multi-team/prompts/generate-build-prompts.md` — structured build prompt generation

### 7. Create DASHBOARD.md

Write `multi-team/DASHBOARD.md` with wikilinks to all agents, skills, and config.

---

## Add to Existing Project

To add multi-team orchestration to a project that already has code:

### 1. Symlink or Copy

Option A — Symlink (shared config):
```bash
ln -s ~/projects/vault/multi-team /path/to/project/multi-team
ln -s ~/projects/vault/.claude /path/to/project/.claude
cp ~/projects/vault/CLAUDE.md /path/to/project/CLAUDE.md
```

Option B — Copy (independent config):
```bash
cp -r ~/projects/vault/multi-team /path/to/project/multi-team
cp -r ~/projects/vault/.claude /path/to/project/.claude
cp ~/projects/vault/CLAUDE.md /path/to/project/CLAUDE.md
```

### 2. Customize Domains

Edit the worker agent `.md` files to match the project's directory structure:
- Backend Dev → adjust `domain.write` to match where backend code lives
- Frontend Dev → adjust `domain.write` to match where frontend code lives
- QA Engineer → adjust `domain.write` to match where tests live

### 3. Customize Teams

If the project doesn't need all three teams, remove teams from `config.yaml`. If it needs additional teams (e.g., data, infrastructure, mobile), use `/agents create` or the agent-creator skill.

### 4. Create Project-Specific Domain Directories

```bash
mkdir -p src/backend src/frontend tests/backend tests/frontend tests/e2e specs
```

---

## Verification

After setup, run this test:

```
/team Ping each team lead and have them confirm their team members.
```

Expected behavior:
1. Orchestrator reads config.yaml
2. Delegates to each lead via subagent
3. Each lead reports their team members
4. Orchestrator synthesizes and presents the result

If this works, the system is properly configured.

---

## Troubleshooting

| Issue | Fix |
|-------|-----|
| `/team` doesn't work | Check `.claude/commands/team.md` exists |
| Config not found | Verify `multi-team/config.yaml` path |
| Agent file missing | Check `system_prompt` paths in config.yaml are relative to `multi-team/` |
| Skills not loading | Verify skill names in front matter match directory names in `multi-team/skills/` |
| Domain enforcement not working | Check `.claude/rules/multi-team.md` exists |
| Expertise not persisting | Verify expertise file paths in agent front matter |
