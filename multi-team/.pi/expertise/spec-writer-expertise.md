# Spec Writer Expertise

*This file is maintained by the spec writer agent. Do not edit manually.*

<!-- 
This file will grow over time as the agent works.
The agent tracks: patterns, decisions, risks, mistakes, and insights.
Max lines: 5000
-->

---

## Session: 2026-04-06 — Arbitrage Bot Deployment Strategy

### Task
Wrote `specs/deployment-strategy.md` — a 1556-line, 62KB comprehensive deployment specification from 6 ADRs.

### What Worked Well

**Reading strategy**: Parallelized all 4 source file reads in one call (deployment-decisions.md, Cargo.toml, .env.example, config/default.toml + main.rs). Saved multiple round-trips. Always batch independent reads.

**Spec completeness pattern**: When producing an actionable spec from ADRs, include:
1. Rationale pointer (why this decision, per ADR-X)
2. Complete, copy-paste-ready code block
3. Verification command(s) to confirm it worked
4. What NOT to do (alternatives rejected, and why)

**cargo-chef Dockerfile pattern** (for Rust multi-stage builds):
```dockerfile
FROM rust:1.85-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json   # deps layer — cached
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
RUN cargo build --release --bin arb                       # src layer — fast

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/arb .
COPY config/ config/
RUN mkdir -p data secrets
ENTRYPOINT ["./arb", "--headless"]
```

**docker-compose.yml volume mount for secrets**: The PEM key override pattern is critical:
```yaml
env_file: secrets/.env          # loads KALSHI_PRIVATE_KEY_PATH=./kalshi_private.pem
environment:
  - KALSHI_PRIVATE_KEY_PATH=/app/secrets/kalshi_private.pem  # overrides to container path
volumes:
  - ./secrets/kalshi_private.pem:/app/secrets/kalshi_private.pem:ro
```
The env_file sets a relative default; the environment: section overrides to the container-absolute path.

**deploy.sh pattern**: Pre-flight SSH check before slow operations (avoids 10-minute build then failed scp):
```bash
if ! ssh -o ConnectTimeout=5 -o BatchMode=yes "$SERVER_HOST" true &>/dev/null; then
    echo "ERROR: Cannot connect to '$SERVER_HOST' via SSH."
    exit 1
fi
```

### Domain Knowledge: Rust + Docker Specific

- **`panic = "abort"` in release profile**: No stack unwinding; process terminates immediately on panic. Docker's `restart: unless-stopped` handles this correctly — the container exits and restarts.
- **ca-certificates on Debian bookworm-slim**: Required even with `rustls` because `rustls-tls-native-roots` reads the system certificate store. Without it, all HTTPS/WSS connections fail silently.
- **SQLite hot backup**: Use `sqlite3 "$DB_PATH" ".backup '$BACKUP_FILE'"` — NOT `cp`. Raw copy during WAL write produces corrupt backup.
- **config/default.toml format**: The binary checks `config.format` at startup. `"pretty"` = human-readable (dev), `"json"` = structured (production). Must be set before building the Docker image — it's baked in.
- **WORKDIR /app**: The binary uses relative paths for everything (config, DB, .env). WORKDIR is load-bearing — all paths break if changed.

### Gotchas Found During This Task

1. **Volume mount + non-root user**: If adding a non-root user to the Dockerfile (`useradd arb`), the `./data:/app/data` volume mount creates `/app/data` owned by `root` on the host. The arb user can't write to it. Solution: pre-create with `chown 1001:1001 ~/arb-deploy/data` OR skip non-root user for simplicity. I opted to skip non-root in the Dockerfile to avoid this complexity for a solo developer — mentioned it as optional hardening.

2. **`scripts/` directory doesn't exist in the project**: Verified with `ls`. The spec instructs creating it with `mkdir -p scripts`. Document this in the spec, not silently.

3. **`docker-compose` vs `docker compose`**: V2 uses a space (plugin form). All commands in specs should use V2 form.

4. **`crontab -e` vs `/etc/cron.d/`**: Using `crontab -e` (user crontab) is simpler for solo developers than system-level cron files. Avoids permission issues.

### Spec Structure Pattern for Operational Guides

For ops-heavy specs, the right structure is:
- Section 1: Overview (ASCII diagram, requirements table)
- Section 2: All config files (complete, copy-paste-ready)
- Section 3: One-time setup (follow top to bottom)
- Section 4: Secrets (separate section — security-critical)
- Section 5: Runbook (day-to-day ops)
- Section 6: Backup/recovery (disaster scenarios)
- Section 7: Monitoring (observability)
- Section 8+: Advanced topics (migration, alternatives)
- Appendix: Quick reference card (most-used commands)

### Template: Acceptance Criteria Verification

After writing a spec, verify:
```bash
# Check all required sections present
grep "^## " spec-file.md

# Check all required subsections present  
grep "^### " spec-file.md

# Check ADR cross-references
grep "ADR-" spec-file.md | wc -l  # should be > 0 for every major decision

# Check no placeholder values remain
grep -i "your_.*_here\|<TODO>\|PLACEHOLDER\|example\.com" spec-file.md
```

### File Size Reference
- Simple spec (3-5 sections): 200-400 lines
- Medium spec (5-8 sections): 400-800 lines  
- Comprehensive ops guide (9+ sections): 1000-1600 lines
- `deployment-strategy.md`: 1556 lines, 62KB — at the upper end of appropriate
- Meeting prep / Q&A doc: 150-250 lines — keep it scannable, not exhaustive

---

## Session: 2026-04-07 — SUPWISE Exact Online Meeting Prep

### Task
Wrote `specs/exact-meeting-qa.md` — a conversational meeting prep document for an Exact Online integration discussion.

### Domain Constraint Hit
**Target path was outside domain**: The orchestrator asked for output at `/Users/mihail/projects/SUPWISE/docs/v1/exact-meeting-qa.md`. My write domain is `specs/**` and `.pi/expertise/**` only. Wrote to `specs/exact-meeting-qa.md` and flagged the constraint. The orchestrator must route the file to the correct location via an agent that has write access to that project path.

**Pattern**: When a task specifies an output path outside my domain, produce the file in `specs/` with the same filename, and clearly report the constraint in my response so the lead/orchestrator can handle the file move.

### Meeting Prep Doc Pattern

For meeting Q&A prep documents, the right structure is:
1. **Opening pitch** — 3-4 natural sentences, not formal
2. **Wat we al weten** — bullet proof of homework (builds credibility)
3. **Omgeving vragen** — setup questions (package, sandbox, divisions, master data)
4. **Data mapping** — most critical section, GUID problems, edge cases
5. **Technische koppeling** — OAuth, tokens, existing integrations
6. **Proces vragen** — workflow, triggers, downstream effects, edge cases (drop shipments, backorders)
7. **Aandachtspunten** — proactive flags (rate limits, token expiry, migration scope, v1 limitations)
8. **Vervolgstappen** — concrete checklist of what we need after the meeting

### Style Rules Learned (Dutch/English mixed docs)
- Dutch for business/domain terms: klant, leverancier, verkooporder, inkooporder, artikelen, divisie, facturatie
- English for technical terms: GUID, OAuth, API, endpoint, rate limit, token, sandbox, refresh token
- Each question: bold header → 1-line italic rationale → decision tree if relevant
- Decision trees format: "**Als het antwoord X is** → dan doen we Y" (inline, not nested bullet hell)
- NO code blocks in meeting docs — even GUIDs shown as inline text examples
- End with a "last check" note pointing to the 1-2 most critical items

### SUPWISE Domain Knowledge
- SUPWISE is a marine supplies order management system
- Orders have: 1 klant (SO) + N leveranciers (POs) — order 700048 example has 4 leveranciers → 4 POs
- exact_customer_id and exact_supplier_id are TEXT fields, currently holding numeric codes from CSV import
- exact_item_code on articles is NOT unique — multiple SUPWISE articles can share one Exact code
- OAuth2: access token 10min (auto-refresh), refresh token ~30 days inactivity expiry
- Rate limits: 60/min, 5000/day, reset midnight
- Sync statuses: not_synced, synced, sync_failed, outdated
- ~200k artikelen, 24 artikelgroepen
- The GUID resolution (numeric code → UUID) is the #1 blocker before any push can work
- 6 open questions that must surface in any Exact integration meeting:
  1. Welk pakket (W&D vs Manufacturing)
  2. Sandbox beschikbaar? (showstopper if no)
  3. Push knop scope: alleen orders of ook relaties/artikelen?
  4. Drop Shipments?
  5. Sync log visibility (alle users vs admins only)
  6. Backorders: nieuwe order of update?

### What Worked Well
- Framing sections with "Als het antwoord X is, dan..." keeps the doc actionable — reader knows what to do with each answer
- Flagging showstoppers (sandbox, GUIDs) explicitly with ⚠️ and closing note so they don't get lost
- Keeping business justification for every question as a 1-line italic — avoids "why are you asking this?" moments in the meeting
- Checklist format for vervolgstappen with checkboxes — ready to use as action item tracker post-meeting

---

## Session: 2026-04-07 — SUPWISE Phase 5B Exact Online Implementation Plan

### Task
Wrote `specs/phase-5b-implementation-plan.md` — a ~900-line technical implementation plan for Exact Online integration in SUPWISE.

### Context Reading Strategy
Parallelized all 6 context file reads in 2 batches:
- Batch 1: expertise file + exact-koppeling.md + phase-5b-meeting-prep.md
- Batch 2: schema.prisma + app.module.ts + .env.example + config.schema.ts + purchase-orders.service.ts

This saved ~6 serial round-trips.

### Key Design Decisions Made

**GUID column strategy:** Added NEW columns (`exact_account_guid`, `exact_item_guid`) alongside existing legacy fields (`exact_supplier_id`, `exact_customer_id`, `exact_item_code`). Reasoning: no breaking changes to existing code, clear separation between "legacy CSV reference code" and "Exact Online GUID". This is the safe choice.

**One Exact Account per Relation:** A SUPWISE relation that is both supplier and customer maps to ONE Exact Account (single `exact_account_guid`). Flagged as a risk (R5) — needs confirmation if Exact requires separate accounts.

**Article deduplication:** `exact_item_code` is NOT unique in SUPWISE. Group by unique code, POST one Exact Item per unique code, share the GUID across all articles with that code. GUID stored in `exact_item_guid` on each article row.

**API key auth (not OAuth2):** The original spec had OAuth2. Meeting revealed API key from Exact App Center. Flagged R3 as HIGH risk — exact auth header format unconfirmed (direct bearer vs client_credentials grant).

**Async bulk sync:** ~200k articles at 60/min = 56+ hours. Sync runs as fire-and-forget background job. Progress tracked via cursor in `exact_connections` table. Resume on failure by skipping already-synced entities (non-null GUID).

### SUPWISE Domain Knowledge Updates
- `Relation.exactSupplierId` and `exactCustomerId` are TEXT fields holding CSV codes (NOT GUIDs). After Phase 5B they become legacy reference fields.
- `Article.exactItemCode` is NOT unique — multiple articles share same Exact code
- Supabase migrations use SQL (not Prisma migrate) — CHECK constraints and RLS policies are in SQL files
- Pattern: `TEXT + CHECK constraint` preferred over Prisma enums (see `btw_code` precedent)
- `purchase-orders.service.ts` already checks `exactSupplierId` and `exactItemCode` for warnings — will need updating in Phase 5B to check GUID columns instead
- Config schema is Joi-based; optional env vars use `.optional()` not `.required()`
- All module imports follow alphabetical order in `app.module.ts`
- `DatabaseService` is exported from `DatabaseModule` and injected into all services

### Implementation Plan Structure That Worked
For a "from meeting notes → implementable spec" task, the right sections are:
1. **Overzicht** — scope in/out explicitly stated
2. **Architectuurbeslissingen** — compare old spec vs new reality, justify each decision
3. **Database migraties** — full SQL with RLS, indexes, constraints, AND Prisma schema changes
4. **Backend implementatie** — actual TypeScript code (not pseudocode) for each service
5. **API endpoints** — method+path table PLUS detailed request/response shapes for each
6. **Frontend** — UI states per component (table of status → badge → action), ASCII mockups
7. **Implementatievolgorde** — numbered steps with acceptance criteria per step
8. **Risico's** — numbered, severity (HOOG/GEMIDDELD/LAAG), specific mitigation and open question
9. **Bijlagen** — copy-paste payloads for external API calls

### Gotchas
1. **`EXACT_REDIRECT_URI` in .env.example** — needs to be REMOVED since no OAuth2. Flag this explicitly.
2. **Rate limit timing for ~200k records** — always calculate and surface in the spec (200k / 60 = 3333 min = 56h). PMs don't know this without the calculation.
3. **Resume cursor for bulk sync** — critical but easy to overlook. Sync takes hours; any failure must be resumable. Design the cursor into the DB schema from the start.
4. **In-memory rate limiter caveat** — only works for single-instance API. Flag for multi-instance scenarios.
5. **Relation that exists in production Exact but not test** — bulk sync will duplicate if production has the relation. Must check first via GET+filter before POST in production. This is a HIGH risk that's easily overlooked.

### Domain Constraint Hit (again)
Output path `docs/v1/phases/phase-5b-implementation-plan.md` is in the SUPWISE project at `/Users/mihail/projects/SUPWISE/` — outside my write domain. Wrote to `specs/phase-5b-implementation-plan.md` and reported constraint.

### File Size
- Implementation plan: ~900 lines, ~68KB — appropriate for 8-section technical spec with code samples

