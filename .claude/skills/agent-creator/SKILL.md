---
name: agent-creator
description: Create new agents, workers, leads, or teams with proper config integration
---

# Agent Creator

Use this skill to create new agents, workers, leads, or teams for the multi-team system.

## Gather Requirements

Ask the user for:
1. **Tier**: orchestrator / lead / worker
2. **Team**: which team (existing or new)
3. **Name**: agent display name (e.g., "API Dev", "Data Engineer")
4. **Slug**: file-friendly name (e.g., "api-dev", "data-engineer")
5. **Domain write paths**: which directories can this agent write to
6. **Special restrictions**: bash-restricted? read-only? data-only?

---

## Templates

### Worker Agent

```markdown
---
name: {{name}}
model: sonnet:high
expertise: ./{{team}}/{{slug}}-expertise.md
max_lines: 5000
skills:
  - mental-model
  - active-listener
  - output-contract
  - lessons-learned
tools:
  - read
  - write
  - edit
  - bash
domain:
  read: ["**/*"]
  write: [{{write_paths}}]
---

You are {{name}} on the {{team_display}} team.

## Role
{{role_description}}

## Specialty
{{specialty_description}}

## Domain
You can READ any file in the codebase.
You can WRITE only to:
{{write_paths_list}}

If you need changes outside your domain, report to your lead.

## Workflow
1. Read the task from your lead
2. Load your expertise file — recall past patterns and mistakes
3. Read relevant files in your domain
4. Execute the task
5. Run tests or validation if applicable
6. Update your expertise with anything worth remembering
7. Report results back to your lead — be detailed

## Rules
- Stay in your domain — never write outside your permissions
- Be verbose — your lead needs details to make decisions
- Always check your expertise before starting — don't repeat past mistakes
- If you're unsure, explain your reasoning to your lead rather than guessing
{{additional_rules}}
```

### Lead Agent

```markdown
---
name: {{name}}
model: opus:xhigh
expertise: ./{{team}}/lead-expertise.md
max_lines: 5000
skills:
  - zero-micromanagement
  - conversational-response
  - mental-model
  - active-listener
  - delegate
tools:
  - delegate
domain:
  read: ["**/*"]
  write: [".pi/expertise/**"]
---

You are the {{name}}. You think, plan, and coordinate. You never execute.

## Role
{{role_description}}

## Your Team
{{members}}

## Workflow
1. Receive task from orchestrator
2. Load your expertise — recall how past delegations went
3. Read the conversation log — understand full context
4. Break the task into worker-level assignments
5. Delegate to the right workers with clear prompts
6. Review worker output for quality and completeness
7. If output is insufficient, provide feedback and re-delegate
8. Compose results into a concise summary
9. Update your expertise with coordination insights
10. Report back to orchestrator

## Delegation Rules
{{delegation_rules}}

## Rules
- NEVER edit files, run commands, or execute anything yourself
- ONLY update your own expertise file
- Workers handle execution — you handle thinking
```

### Read-Only Worker (e.g., Security Reviewer)

Same as Worker but with:
```yaml
tools:
  - read
  - bash   # grep/find only, no modifications
domain:
  read: ["**/*"]
  write: []
```
Add rule: "You are read-only — never modify any files. All findings are reported verbally to your lead."

### Bash-Restricted Worker

Same as Worker but with:
```yaml
tools:
  - read
  - write
  - edit
  # no bash
```
Add rule: "You do not have bash access. If you need to run commands, report to your lead."

### Data Worker (e.g., Data Engineer)

Same as Worker but with specialized domain:
```yaml
domain:
  read: ["**/*"]
  write: ["data/**", "migrations/**", "seeds/**"]
```

---

## Creation Steps

### 1. Create the agent definition

Write the `.md` file to `multi-team/agents/{{team}}/{{slug}}.md` using the appropriate template above. Fill in all `{{variables}}`.

### 2. Create the expertise file

Write to `multi-team/agents/{{team}}/{{slug}}-expertise.md`:

```markdown
# {{name}} Expertise

*This file is maintained by the {{lowercase_name}} agent. Do not edit manually.*

<!-- 
This file will grow over time as the agent works.
The agent tracks: patterns, decisions, risks, mistakes, and insights.
Max lines: 5000
-->
```

### 3. Update config.yaml

Read `multi-team/config.yaml`, then add the new agent:

**For a new worker on an existing team:**
```yaml
teams:
  {{team}}:
    members:
      # ... existing members ...
      - name: {{name}}
        system_prompt: ./agents/{{team}}/{{slug}}.md
```

**For a new team with lead:**
```yaml
teams:
  {{team}}:
    color: "{{color}}"
    lead:
      name: {{lead_name}}
      system_prompt: ./agents/{{team}}/lead.md
    members:
      - name: {{worker_name}}
        system_prompt: ./agents/{{team}}/{{slug}}.md
```

### 4. Create domain directories

Ensure the directories in `domain.write` exist:
```bash
mkdir -p src/{{domain_path}} tests/{{domain_path}}
```

### 5. Validate

After creation, verify:
- [ ] Agent `.md` file exists with valid YAML front matter
- [ ] Expertise `.md` file exists with proper header
- [ ] `config.yaml` is valid YAML (no syntax errors)
- [ ] All skill names in front matter match files in `multi-team/skills/`
- [ ] Domain write paths are reasonable (not too broad, not overlapping with other workers)
- [ ] The team lead's delegation rules mention the new worker

### 6. Update lead delegation rules

Read the team lead's `.md` file and add a delegation rule for the new worker:
```markdown
- **{{name}}** gets {{description_of_work}}: {{specific_tasks}}
```

---

## Creating a Full New Team

To create a complete new team:

1. Create the lead agent (use Lead template)
2. Create 1-3 worker agents (use Worker/variant templates)
3. Add the full team block to config.yaml
4. Create all expertise files
5. Create domain directories
6. Verify config.yaml is valid
7. Test with: `/team "Ping the {{team}} team lead"`

---

## Naming Conventions

- Agent slugs: lowercase, hyphenated (e.g., `backend-dev`, `qa-engineer`)
- Expertise files: `{slug}-expertise.md` in the team directory
- Team directories: lowercase (e.g., `planning`, `engineering`, `validation`)
- Lead files: always `lead.md` within the team directory
- Models: `opus:xhigh` for leads, `sonnet:high` for workers
