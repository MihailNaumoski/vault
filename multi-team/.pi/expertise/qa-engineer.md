# QA Engineer Expertise

*This file is maintained by the QA engineer agent. Do not edit manually.*

## Session: 2026-04-07 — Exact Online Phase 5B Review

### Task type
Read-only haalbaarheidsanalyse (geen tests). Schema-validatie, berekeningen, edge-case review.

### Codebase context
- NestJS + Prisma ORM + Supabase (PostgreSQL)
- Schema: `/Users/mihail/projects/SUPWISE/packages/database/prisma/schema.prisma`
- Plan: `/Users/mihail/projects/SUPWISE/docs/v1/phases/phase-5b-implementation-plan.md`
- API-analyse: `/Users/mihail/Downloads/Exact Online API Documentatie Analyse.md`
- Spec: `/Users/mihail/projects/SUPWISE/docs/v1/exact-koppeling.md`

### Key findings — Exact Online integratie

#### Bulk sync rate limits (V2)
- 5000 calls/dag is de harde grens, NIET 60/min
- 60/min is theoretisch sneller bereikt (833 min voor 50k calls), maar de daggrens is altijd bottleneck
- Mandatory $filter geldt ALLEEN voor GET, niet voor POST — dit is een veelgemaakte veronderstelling
- Sync API (GET 1000/call) kan alleen gebruikt worden voor LEZEN, niet voor SCHRIJVEN
- Error Velocity limit (10 fouten/endpoint/uur) is gevaarlijk bij bulk loops met validatiefouten

#### Schema patronen
- Prisma UUID velden: `String? @db.Uuid` is het standaard Prisma patroon voor PostgreSQL UUID types
- `exact_sync_log.entity_type` CHECK: in de spec staat `article`/`relation`, in SQL staat `account`/`item` — let op inconsistentie
- Triggers op `orders` missen `order_lines` wijzigingen — price/qty changes triggeren geen 'outdated'
- `updated_at DEFAULT now()` in raw SQL zonder trigger update niet automatisch — Prisma `@updatedAt` doet dit wel

#### Order push
- SalesOrderLines en PurchaseOrderLines zijn beiden INLINE in de POST → 1 call per SO/PO ✅
- Push service mist check op bestaand `exactSalesOrderId` → duplicate SO risico bij retry
- `exact_purchase_order_number` kolom wordt aangemaakt maar nooit gevuld in de service code

#### RLS policies
- Supabase: service_role bypasses RLS altijd. `USING (true)` geeft ook authenticated users volledige toegang.
- Voor security-sensitive tabellen: `USING (auth.role() = 'service_role')` is veiliger

### Patterns om te onthouden
- Bij Exact Online: mandatory $filter = alleen GET, POST onbeperkt
- Exact rate limits: dag-limiet is de bindende constraint bij bulk (niet per-minuut)
- Sync API is read-only (GET) — geen bulk POST equivalent
- Prisma `updateMany` with partial index is efficient als de index de WHERE kolom dekt
