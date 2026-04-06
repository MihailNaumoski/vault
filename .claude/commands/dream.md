Load the mental-model skill from `.claude/skills/mental-model/SKILL.md` and run expertise consolidation across all agents.

## Steps

1. Read the mental-model skill for consolidation guidelines
2. Find all `*-expertise.md` files in `multi-team/agents/` (including subdirectories)
3. For each file, read the corresponding agent's `.md` front matter to get `max_lines`
4. For each expertise file with content beyond the template header:
   a. Check for stale entries (reference files/patterns that no longer exist)
   b. Check for duplicate entries (same insight stated differently)
   c. Check for verbose entries (line-by-line details instead of high-level patterns)
   d. Check line budget usage (current lines vs max_lines)
5. Consolidate: remove stale, merge duplicates, condense verbose, reorder by topic
6. Write back consolidated files (preserve the header format)
7. Report all changes

## Output

Produce the consolidation report as described in the mental-model skill:
- Table with before/after line counts, budget usage, entries removed/merged
- Detailed changes per agent
- Recommendations for agents with high budget usage or empty files

## Rules

- Never remove entries without verifying staleness (check if referenced files still exist)
- Preserve each agent's voice — tighten, don't rewrite
- Keep the file header intact: `# {Name} Expertise` + italics warning + comment block
- If a file is empty (just the template), skip it and note "no entries yet"
