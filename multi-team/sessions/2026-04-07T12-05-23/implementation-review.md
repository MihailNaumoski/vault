# Phase 5B Implementation Plan -- Code Review

**Reviewer:** Code Reviewer (Engineering Team)
**Date:** 2026-04-07
**Reviewed:** `implementation-plan.md` for Phase 5B Exact Online Integration

---

## Overall Assessment: APPROVED with minor items

The implementation plan is thorough, well-aligned with the existing SUPWISE codebase patterns, and correctly handles the critical Exact Online API constraints. The API key auth approach (not OAuth2) is correctly implemented throughout. Below are findings organized by category.

---

## 1. Correctness Against Exact Online API Constraints

### PASS: Rate Limiting

- Dual header monitoring is correctly implemented. The plan correctly identifies that Exact *switches* between daily and minutely headers (not sends both simultaneously). The `updateRateLimitFromHeaders()` method correctly prioritizes minutely headers when present.
- Soft limits (55/min, 4950/day) provide appropriate safety margin against the hard limits (60/min, 5000/day).
- Daily counter is DB-persisted (`exact_connections.daily_calls_used`), surviving server restarts. Good.

### PASS: Error Velocity / Circuit Breaker

- The 7-error-per-endpoint threshold is conservative enough (Exact blocks at 10). The 1-hour sliding window matches Exact's enforcement window.
- Circuit breaker correctly prevents further calls when open, with clear error messages.
- Both sync services correctly check for circuit breaker errors and abort the sync loop.

### PASS: Error Parsing

- Correctly notes that `error.code` is always empty and parses `error.message.value`.
- Correctly identifies 200 OK with empty results as a permission issue (logs a warning).

### PASS: No $expand Workaround

- The plan does not attempt to use `$expand` (correctly, since Exact doesn't support it).
- POST responses return inline created entities -- the plan correctly extracts GUIDs from `response.d`.

### MINOR ITEM: Mandatory $filter on GET endpoints

- The plan uses `getAll()` for potential reads, but the validation service also does reads from the local database (not from Exact).
- If the team later adds read-from-Exact features, remember that `crm/Accounts`, `logistics/Items`, and `salesorder/SalesOrders` require `$filter` on GET. The current `getAll()` method supports `params.filter`, which is good.

---

## 2. Security Review

### PASS: API Key Storage

- AES-256-GCM with random IV per connection is correct. Auth tag prevents tampering.
- Auth tag stored with ciphertext using `:` separator -- works, but consider using a structured format (JSON) if the schema ever needs to store additional crypto metadata.
- `EXACT_ENCRYPTION_KEY` is hex-validated in the Joi config schema (`.hex().length(64)`).

### PASS: No Credential Leakage

- Pino redaction includes `req.body.apiKey`.
- `getStatus()` explicitly uses `select:` to exclude `apiKeyEnc` and `encryptionIv`.
- Sync log stores request/response bodies but the API key is only in the Authorization header (already redacted by Pino at line 41 of `app.module.ts`).
- Audit logs never include the API key (only `divisionId`).

### PASS: RLS Policies

- Both `exact_connections` and `exact_sync_log` have RLS enabled with service-role-only policies.
- This prevents any client-side Supabase access to these sensitive tables.

### PASS: Role-Based Access

- Admin-only for connection management and sync.
- Admin + manager_senior for push operations -- matches the existing `QUOTE_ORDER_ROLES` pattern for order management.

---

## 3. Consistency with Existing Codebase Patterns

### PASS: Module Structure

- Follows the standard `module.ts -> controller.ts -> service.ts -> DatabaseService` pattern.
- Uses `@nestjs/axios` HttpModule (already in `package.json` at line 17).
- Three separate controllers follow the pattern of separating concerns (like `OrdersController` handles orders, `OrderLinesService` handles lines).

### PASS: DTO Validation

- Uses `class-validator` decorators consistently with existing DTOs (compare `CreateOrderDto` at lines 1-53).
- `ConnectExactDto` follows the same `@IsString()`, `@MinLength()`, `@MaxLength()` pattern.

### PASS: Database Interaction

- Uses `DatabaseService` (Prisma wrapper) consistently.
- Transaction pattern in `ExactAuthService.connect()` matches `OrdersService.create()` (line 278).
- `logAudit()` calls follow the exact pattern from `OrdersService` (line 308-313).
- Batch updates with `updateMany()` follow `ArticlesImportService` pattern.

### PASS: Error Handling

- Uses `handlePrismaError` pattern implicitly via try/catch + throw.
- NestJS exceptions (`NotFoundException`, `BadRequestException`, `UnauthorizedException`) match existing usage.

### PASS: Prisma Schema Conventions

- All models use `@@map("snake_case")` for table names.
- All fields use `@map("snake_case")` for column names.
- UUID fields correctly annotated with `@db.Uuid`.
- DateTime fields use `@db.Timestamptz`.
- Index annotations follow existing patterns.

---

## 4. Edge Cases and Missing Items

### ITEM 1: Duplicate Push Protection (LOW risk)

The validation service warns when `exactSalesOrderId` is already set, but it's only a warning, not a blocker. Consider making this a hard error in v1 to prevent accidental duplicates in Exact. The user can always clear the Exact ID and re-push if needed.

**Recommendation:** Add a hard check in `pushOrder()`:
```typescript
if (order.exactSalesOrderId) {
  throw new BadRequestException('Order already pushed to Exact. Use the outdated re-sync flow.');
}
```

### ITEM 2: Transaction Safety on Order Push (MEDIUM risk)

The current `pushOrder()` updates the DB after each API call. If the server crashes between the SO push and PO pushes, the order will be marked as `synced` with no POs in Exact. This is acceptable for v1 (the user sees POs as `not_synced` and can retry), but document this as a known limitation.

**Recommendation:** The current design is pragmatic. A full saga pattern would be over-engineering for v1. The retry endpoint handles this case.

### ITEM 3: Article Deduplication Edge Case (LOW risk)

When multiple articles share the same `exact_item_code` but have different descriptions, the plan uses the first article's description for the Exact Item. This is documented in the existing plan as risk R4. The implementation correctly uses `updateMany()` to assign the GUID to all articles.

**Recommendation:** Log a warning when a code group has articles with different descriptions. Already partially addressed.

### ITEM 4: Sync Resume After Interruption (LOW risk)

If a sync is interrupted (server restart, circuit breaker), the sync is naturally resumable because it filters on `exactAccountGuid: null` / `exactItemGuid: null`. Already-synced entities are automatically skipped. Good idempotent design.

### ITEM 5: Order Line Trigger (NEW -- good addition)

The plan adds a trigger `trg_order_line_exact_outdated` that marks the parent order as `outdated` when lines are inserted/updated/deleted. This is a good addition beyond the original plan, which only triggered on order-level field changes. This catches the edge case where a user adds/removes lines after syncing.

### ITEM 6: `exact_purchase_line_id` Column (NEW -- good addition)

The plan adds GUID columns on `purchase_order_lines` as well, not just `order_lines`. This allows line-level tracking for both SO and PO lines.

### ITEM 7: `ExactConnection` Relation on UserProfile (MINOR)

The plan adds `exactConnections` and `exactSyncLogs` relations to the `UserProfile` model. Make sure to also add the corresponding `@relation` annotation on the `ExactConnection` model to specify the relation name, matching the pattern used elsewhere (e.g., `PurchaseOrder` uses `@relation("PurchaseOrderCreator")`).

### ITEM 8: Config Schema -- EXACT_ENCRYPTION_KEY is `.optional()` (VERIFY)

The Joi schema marks `EXACT_ENCRYPTION_KEY` as optional. This is correct for development (Phase 5B might not be deployed immediately), but `ExactAuthService.getEncryptionKey()` throws a hard error if missing. This is fine -- it means the feature is opt-in.

---

## 5. Missing from Plan (not critical for v1)

1. **No Playwright test specifications** -- The existing codebase has `tests/testphases/phase5a-*` tests. Consider adding `phase5b-*` test specs.
2. **No SSE/WebSocket for sync progress** -- Frontend polls via `GET /sync/progress/:jobId`. For v1, polling is fine. Consider SSE for v2.
3. **No automatic daily counter reset** -- The daily counter is persisted but only reset when a header indicates a new day. If no API calls are made on a new day, the counter remains stale. This is benign (it will reset on the first call of the new day).

---

## 6. Summary of Action Items

| # | Priority | Item | Action |
|---|----------|------|--------|
| 1 | MEDIUM | Duplicate push protection | Make `exactSalesOrderId` check a hard error, not warning |
| 2 | LOW | UserProfile relation annotation | Add `@relation` name to `ExactConnection.connectedByUser` |
| 3 | LOW | Description mismatch logging | Add explicit warning log when dedup group has differing descriptions |
| 4 | NONE | Transaction safety on push | Document as known limitation, retry endpoint covers the gap |
| 5 | NONE | Playwright test specs | Add in a follow-up task, not blocking for implementation |

---

## Verdict

**APPROVED.** The plan is comprehensive, correctly handles all known Exact Online API constraints, follows SUPWISE's established codebase patterns, and has good security posture. The three controller split, pre-flight validation service, and circuit breaker are particularly well-designed additions that go beyond the original plan. Proceed with implementation starting at Step 1 (SQL migration).
