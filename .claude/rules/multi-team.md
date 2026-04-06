# Multi-Team Domain Enforcement Rules

These rules are always active in this vault.

## Domain Enforcement

- When operating as a delegated agent (subagent), ALWAYS check your domain from your YAML front matter before writing any file
- Match the file path against your `domain.write` glob patterns — if no pattern matches, the write is FORBIDDEN
- If a write is outside your domain, stop and report to your lead — do not attempt the write

## Lead Agent Rules

- If you are a lead agent (has `zero-micromanagement` skill or `tools: [delegate]`), NEVER write files or run bash commands
- Leads ONLY delegate via subagents and update their own expertise file
- If you catch yourself about to write a file as a lead, stop and delegate to a worker instead

## Worker Agent Rules

- If you are a worker agent, NEVER write to directories outside your `domain.write` patterns
- Before every file write, verify the target path matches at least one pattern in your `domain.write`
- If you need changes outside your domain, report the need to your lead with specific file paths

## Conversation Log Protocol

- Always append to the session `conversation.jsonl` after completing work
- Always read your expertise file before starting any task
- Always update your expertise file after learning something significant

## Expertise File Ownership

- Expertise files with the header "*This file is maintained by the {agent} agent. Do not edit manually.*" are agent-maintained
- Humans should not edit these files — use `/dream` for consolidation instead
- Each agent owns exactly one expertise file — never update another agent's file

## Delegation Protocol

- When delegating to a subagent, always include:
  1. The specific task with clear acceptance criteria
  2. Which files and directories are involved
  3. Domain restrictions (explicit `domain.write` paths)
  4. Context from the conversation log
  5. The agent's expertise file content
