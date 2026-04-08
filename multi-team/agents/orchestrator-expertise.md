# Orchestrator Expertise

*This file is maintained by the orchestrator agent. Do not edit manually.*

<!-- 
This file will grow over time as the agent works.
The agent tracks: patterns, decisions, risks, mistakes, and insights.
Max lines: 10000
-->

### ALWAYS Use Full Three-Tier Hierarchy — 2026-04-06
- **Context**: User explicitly requested that all delegations go through leads. No single-worker shortcuts.
- **Insight**: The full round-trip (Orchestrator → Lead → Worker → Lead → Orchestrator) is required every time. When using /team orchestration, consider ALL teams (Planning, Engineering, Validation) — not just Engineering.
- **Action**: Never skip the lead tier. Never delegate directly to a worker. Always route through the team lead, and the lead always reports back to the orchestrator. On /team invocations, evaluate whether Planning should review the approach and Validation should review the output.

### Domain Adaptation for Sub-Projects — 2026-04-06
- **Context**: arbitrage-trader lives in `projects/arbitrage-trader/crates/**`, not `src/backend/**`
- **Insight**: Worker domain.write patterns are defaults for a generic project layout. When working on sub-projects with different structures, adapt the domain at delegation time.
- **Action**: Always check the actual project structure before delegating. Override domain.write in the delegation prompt if the target project doesn't match `src/{frontend,backend}/**`.

### arb-db Tests Use Uuid::now_v7 — 2026-04-06
- **Context**: The arbitrage-trader workspace enables `uuid` with `v7` feature only, not `v4`.
- **Insight**: Test helpers must use `Uuid::now_v7()` instead of `Uuid::new_v4()`.
- **Action**: When delegating DB test writing for this project, note the v7-only constraint.

### Build Prompt Deviations Are Normal — 2026-04-06
- **Context**: Phase 3 build prompt had 4 deviations: clippy doc-comment style, --match CLI ordering (moved before DB init), store.rs adapted to actual schema, chrono::Duration consistency.
- **Insight**: Even well-specified prompts need runtime adaptation. The deviations were all correct judgment calls. Backend Dev correctly prioritized "compiles and passes clippy" over "matches prompt exactly."
- **Action**: When delegating from specified build prompts, explicitly tell workers to adapt code to compile against the real codebase, not follow the prompt blindly. Include "verify against actual API signatures" in all specified-code delegations.

### Pre-Analyze Build Prompts Before Delegating — 2026-04-06
- **Context**: Phase 4 had known deviations (MockState import paths, async lock holding patterns). Pre-identifying these and passing to the Engineering Lead saved debugging time.
- **Action**: Before delegating build prompts, analyze the prompt against the real codebase. Identify import path mismatches, trait method signatures, and feature gate patterns. Pass these as "known deviations" to the lead.

### Planning-First for Specified Build Prompts Works Well — 2026-04-06
- **Context**: Phase 5 had a detailed build prompt with near-complete code. Running Planning (Architect + Spec Writer) first identified 7 deviations (3 blocking) before Engineering touched any code.
- **Insight**: Even when the build prompt has code, the Planning team adds value by verifying API contracts against the real codebase and producing a spec with numbered ACs that the Code Reviewer and Validation can audit against.
- **Action**: For specified build prompts, always run Planning first to produce plan.md + spec.md. The spec's AC table becomes the single source of truth for all downstream teams.

### Parallel QA + Security for Validation — 2026-04-06
- **Context**: Phase 5 validation delegated QA Engineer and Security Reviewer in parallel since they have no dependencies on each other. Both completed and reported back efficiently.
- **Action**: Always run QA and Security in parallel during Validation. They read the same files but check different things.

### Research → Engineering Pipeline — 2026-04-06
- **Context**: Added a dedicated Research team (Lead + Doc Researcher + SDK Analyst) to handle API investigation before Engineering starts.
- **Insight**: Research team produces a `research-handoff.md` that Engineering consumes directly. The handoff has exact endpoints, JSON formats, auth details, SDK references, and an implementation checklist. Engineering should NOT need to open a browser.
- **Action**: For any task involving a new external API: (1) Delegate to Research Lead first, (2) Wait for `research-handoff.md`, (3) Pass the handoff path to Engineering Lead. Research and Planning can run in parallel if Planning doesn't depend on API details.

### Research Team — Parallel Workers — 2026-04-06
- **Context**: Research team has Doc Researcher (reads official docs/guides) and SDK Analyst (reads SDK source code from GitHub).
- **Insight**: Run them in parallel — they answer different questions. Doc Researcher gets the "what" (endpoints, params, rate limits). SDK Analyst gets the "how" (exact serialization, auth implementation, message types). When docs and SDK disagree, the SDK wins.
- **Action**: Always delegate BOTH workers for API research. The Research Lead synthesizes their output into the handoff doc.

### API Research Phases Work Well — 2026-04-06
- **Context**: Phase 6 (Polymarket API fix) split into 2 phases: Research/Plan then Implement/Validate. The Architect fetched external docs via WebFetch and verified API patterns before Engineering started.
- **Insight**: For tasks requiring external API integration, a dedicated Research phase before Implementation prevents wasted engineering time. The Architect discovered 5 critical deviations from assumptions (WS subscription format, heartbeat format, PriceChange array structure, subscription field grouping, token ID mapping strategy).
- **Action**: When a task involves external API integration where the docs haven't been read yet, always run a Research phase (Planning team with WebFetch) before Engineering. The research findings become the single source of truth for implementation.

### Skip Playwright/Frontend for Rust Backend Projects — 2026-04-06
- **Context**: Phase 6 was a Rust workspace with no frontend. Skipped Frontend Dev and Playwright Tester — only Backend Dev and Code Reviewer ran.
- **Action**: For Rust-only backend projects, Engineering delegation should only include Backend Dev and Code Reviewer. No Frontend Dev, no Playwright Tester.

### Context Loader Can Be Lightweight for Phase 2 — 2026-04-06
- **Context**: Phase 2 skipped Context Loader because Phase 1 plan.md + spec.md already contained everything Engineering needed. Running Context Loader would have been redundant.
- **Action**: If Phase 1 produces comprehensive plan.md + spec.md, skip Context Loader for Phase 2 and provide the plan/spec paths directly in the Engineering Lead prompt.

### Kalshi Research Revealed 4 Breaking Issues — 2026-04-06
- **Context**: arb-kalshi crate was built from assumptions. Research team (Doc Researcher + SDK Analyst parallel) found 4 breaking issues: wrong signing algo (PKCS1v15 instead of PSS), wrong base URLs (trading-api.kalshi.com → api.elections.kalshi.com), wrong WS auth (JSON message instead of HTTP upgrade headers), wrong WS message envelope (flat vs wrapped).
- **Insight**: Even code that "looks right" can have fundamental protocol errors. The official Python starter code (github.com/Kalshi/kalshi-starter-code-python) was the authoritative source for auth implementation.
- **Action**: For any exchange connector, ALWAYS run Research team before Engineering implements. The cost of building on wrong assumptions is much higher than the cost of research.

### Research-Only Delegations Work Well — 2026-04-06
- **Context**: Used Research team only (no Planning/Engineering/Validation) for a pure research task. Doc Researcher + SDK Analyst ran in parallel, Research Lead synthesized.
- **Insight**: Research-only delegation is clean and fast. The handoff document format (research-handoff.md) is proven effective — covers endpoints, auth, WS protocol, comparison, implementation checklist.
- **Action**: For "research X API" tasks, delegate Research team only. Save Planning for when there's a spec to write, Engineering for when there's code to build.

### Research → Engineering Pipeline Proven End-to-End — 2026-04-06
- **Context**: Kalshi API fix used Research team first (found 4 breaking + 8 suboptimal issues), then Engineering team (Backend Dev + Code Reviewer). The handoff doc was the sole interface between teams.
- **Insight**: The pipeline worked smoothly. Engineering needed zero web access — everything was in the handoff. Code Reviewer also found a pre-existing bug (query params in signing path) that Research hadn't caught. The two-phase approach (Research → Engineering) is strictly better than single-phase.
- **Action**: For external API fixes: (1) Research team produces handoff, (2) Engineering consumes handoff. Never combine into one delegation.

### Code Review Catches Pre-Existing Bugs — 2026-04-06
- **Context**: Code Reviewer found that `fetch_markets` included query parameters in the signing path, violating the Kalshi spec. This bug existed before the current changes.
- **Insight**: Code review is not just about reviewing new code — the reviewer should also check adjacent code that touches the same systems.
- **Action**: Always include Code Reviewer in Engineering delegations, even for seemingly simple changes.

### Research + Planning Parallel for UI/UX Tasks — 2026-04-08
- **Context**: TUI chart redesign task. Research team investigated ratatui techniques while Planning team designed the UX/architecture. Then Trading team implemented.
- **Insight**: For UI/UX redesign tasks, Research (what's possible technically) and Planning (what should we build) can run in parallel because they answer orthogonal questions. The Planning team's Architect reads the current codebase to understand constraints, while Research reads external docs/examples for techniques.
- **Action**: For UI/UX tasks: run Research + Planning in parallel, then hand both outputs to the implementing team (Trading or Engineering depending on domain).

### Trading Team Owns TUI Implementation — 2026-04-08
- **Context**: The TUI lives in arb-cli which is in the Trading team's Rust Engine Dev domain. Engineering team was not needed for this Rust-only TUI change.
- **Action**: For arb-cli/TUI changes, delegate to Trading Lead → Rust Engine Dev. Engineering team (Backend/Frontend Dev) is for web-project-style code, not Rust TUI work.

### Skip tachyonfx for Initial Implementation — 2026-04-08
- **Context**: Research found tachyonfx (shader-like effects for ratatui). Planning spec marked it optional (AC-10.5 feature flag). Trading team skipped it, focusing on core chart redesign. Correct call — 1903 lines was already a substantial rewrite.
- **Action**: When a research phase discovers "nice to have" crate additions, don't burden the first implementation pass. Implement core functionality first, effects/polish in a follow-up phase.

### Mock Connector One-Shot Failure Semantics — 2026-04-06
- **Context**: Both mock connectors (Polymarket/Kalshi) use a one-shot `should_fail` field that is consumed by the FIRST call to any method (including `get_balance()`). The build prompt assumed failures would hit `place_limit_order()`, but the executor calls `get_balance()` first during risk checks, consuming the failure before the order placement.
- **Insight**: Test code that injects failures must account for the call ordering in the system under test. A wrapper struct that fails only on specific methods is more reliable than one-shot injection.
- **Action**: When delegating tests involving mock failures, note this constraint: "Mock failures are one-shot and consumed by the first call — if the code under test calls get_balance() before place_limit_order(), the failure won't reach the intended method."
