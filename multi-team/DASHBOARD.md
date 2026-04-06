# Multi-Team Agentic Coding Vault

> Home base for orchestrating multi-agent coding workflows.

## Orchestrator

- [[orchestrator]] --- owns the conversation, delegates everything
- [[orchestrator-expertise]] --- agent-maintained knowledge

## Teams

### [[Planning|Planning Team]]
- [[planning/lead|Planning Lead]] · [[planning/lead-expertise]]
- [[planning/architect|Architect]] · [[planning/architect-expertise]]
- [[planning/spec-writer|Spec Writer]] · [[planning/spec-writer-expertise]]

### [[Engineering|Engineering Team]]
- [[engineering/lead|Engineering Lead]] · [[engineering/lead-expertise]]
- [[engineering/backend-dev|Backend Dev]] · [[engineering/backend-dev-expertise]]
- [[engineering/frontend-dev|Frontend Dev]] · [[engineering/frontend-dev-expertise]]

### [[Validation|Validation Team]]
- [[validation/lead|Validation Lead]] · [[validation/lead-expertise]]
- [[validation/qa-engineer|QA Engineer]] · [[validation/qa-engineer-expertise]]
- [[validation/security-reviewer|Security Reviewer]] · [[validation/security-reviewer-expertise]]

## Configuration

- [[config]] --- team structure, models, resource allocation
- [[plan-build-validate]] --- reusable workflow template
- [[generate-build-prompts]] --- generate structured build prompts for a phase (ported from SUPWISE)

## Skills

- [[mental-model]] --- how agents maintain expertise files
- [[output-contract]] --- declare files, exports, and build status per task
- [[self-validation]] --- validate your own output before presenting
- [[lessons-learned]] --- mine past reviews and incidents for repeatable patterns
- [[zero-micromanagement]] --- leads never execute, always delegate
- [[conversational-response]] --- concise response patterns
- [[active-listener]] --- read conversation log before responding
- [[delegate]] --- how to delegate work effectively

## Knowledge Base

- [[tac-notes]] --- Tactical Agentic Coding notes
- [[claude-code-leak]] --- Claude Code architecture notes
- [[pi-architecture]] --- Pi agent notes

## Projects

- [[SUPWISE]] --- ERP / WeSupply integration
- [[home-server]] --- Home server project
- [[ais-tracker]] --- AIS tracker project

## Templates

- [[new-agent-worker]] --- create new worker agents
- [[new-agent-lead]] --- create new lead agents
- [[new-expertise]] --- initialize expertise files

## Quick Actions

```
just team          # launch orchestrator
just teams         # list teams
just agents        # list all agents
just stats         # expertise file sizes
just new-session   # start a new session
```
