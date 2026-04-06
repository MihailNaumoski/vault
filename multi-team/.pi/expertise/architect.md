# Architect Expertise

*This file is maintained by the architect agent. Do not edit manually.*

## Session: 2026-04-06 — Phase-Based Workflow Architecture

### Patterns Identified
- Current system: 3 teams (planning, engineering, validation), single-pass plan-build-validate
- Agent definitions use YAML frontmatter (name, model, expertise, max_lines, skills, tools, domain)
- Domain enforcement: workers write only within domain.write paths; leads write only to .pi/expertise
- Model tiers: opus:xhigh for reasoning-heavy work, sonnet:high for pattern-based work (QA Engineer already uses sonnet)
- Leads get `delegate` tool only. Workers get `read`, `write`, `edit`, `bash` (some workers are read-only: security-reviewer)

### Decisions Made
1. **Context Loader as standalone agent** — not a team. Fallback: single-member team if framework doesn't support standalone_agents config key
2. **Code Reviewer in Engineering team** — tighter feedback loop. Read-only (can't modify code, only review). Trade-off: same-team review concern, mitigated by read-only domain + Validation team as independent check
3. **Playwright Tester in Engineering** — E2E tests are build artifacts, not just validation. sonnet:high model (pattern-based work)
4. **Per-phase validation** — more expensive but catches issues early. Orchestrator can skip for trivial phases
5. **Phase artifacts in phases/phase-{N}/directory** — structured output per phase with manifest tracking
6. **Orchestrator writes gate decisions and reports** — leads report verbally, orchestrator writes to phases/

### Risks Flagged
- `standalone_agents` may not be a supported pi config key — need framework check
- Orchestrator write domain must expand to `phases/**` — currently only `.pi/expertise/**`
- Cost increase: ~36 agent invocations per 3-phase task vs ~6-8 for single-pass
- Planning agents need expanded write domain to include `phases/**/plan.md` and `phases/**/spec.md`

### Mistakes to Avoid
- Don't assume the pi framework supports arbitrary config keys — always check
- Don't give write access to Code Reviewer — its independence depends on being read-only
- Don't skip the Context Loader for later phases — context drift compounds
