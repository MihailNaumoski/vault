Read `multi-team/config.yaml` and display an overview of all teams.

For each team, show:
- Team name with color indicator
- Lead name and model (from the lead's `.md` front matter)
- All members with their model and domain.write paths (from their `.md` front matter)
- Number of skills per agent

Format as a clean table or structured list. Example:

```
🔵 Planning Team
  Lead: Planning Lead (opus:xhigh) — thinks, plans, coordinates
  Workers:
    • Architect (opus:xhigh) — writes to specs/**
    • Spec Writer (sonnet:high) — writes to specs/**

🟢 Engineering Team
  Lead: Engineering Lead (opus:xhigh) — thinks, plans, coordinates
  Workers:
    • Backend Dev (sonnet:high) — writes to src/backend/**, tests/backend/**
    • Frontend Dev (sonnet:high) — writes to src/frontend/**, tests/frontend/**

🔴 Validation Team
  Lead: Validation Lead (opus:xhigh) — thinks, plans, coordinates
  Workers:
    • QA Engineer (sonnet:high) — writes to tests/**
    • Security Reviewer (sonnet:high) — read-only
```

Read the actual front matter from each agent's `.md` file to get accurate model and domain info. Don't hardcode — derive from files.

No orchestration needed. Just read and display.
