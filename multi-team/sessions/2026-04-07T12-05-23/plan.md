# Phase 5B — Exact Online Integration: Architecture Plan

**Date:** 2026-04-07
**Status:** FINAL — ready for implementation
**Auth:** API key (confirmed by Exact Online meeting, NOT OAuth2)

---

## 1. Component Overview

### New NestJS Module: `apps/api/src/exact/`

| Service | Responsibility |
|---------|---------------|
| `exact.module.ts` | NestJS module registration, imports, providers |
| `exact.controller.ts` | REST endpoints: connect, disconnect, sync, push, status |
| `exact-auth.service.ts` | API key AES-256-GCM encrypt/decrypt, `exact_connections` CRUD |
| `exact-api.service.ts` | HTTP client wrapper: auth header injection, retry with exponential backoff |
| `exact-rate-limiter.service.ts` | Dual header monitoring (minutely + daily), error velocity tracking, pre-emptive throttling |
| `exact-validation.service.ts` | Pre-flight validation of payloads before API calls (prevents error velocity limit hits) |
| `exact-sync.service.ts` | Bulk sync: relations -> Accounts, articles -> Items, with circuit breaker |
| `exact-push.service.ts` | Order push: builds + sends Sales Order + Purchase Orders to Exact |
| `exact-sync-log.service.ts` | `exact_sync_log` table CRUD, query by entity/status |

### DTOs

| DTO | Purpose |
|-----|---------|
| `connect-exact.dto.ts` | `{ apiKey: string, divisionId: number }` |
| `sync-progress.dto.ts` | `{ total, synced, failed, errors[] }` |
| `push-order.dto.ts` | `{ orderId: string }` |

### Interfaces (Exact API payloads)

| Interface | Maps to |
|-----------|---------|
| `exact-account.interface.ts` | POST `/crm/Accounts` payload + response |
| `exact-item.interface.ts` | POST `/logistics/Items` payload + response |
| `exact-sales-order.interface.ts` | POST `/salesorder/SalesOrders` payload + response |
| `exact-purchase-order.interface.ts` | POST `/purchaseorder/PurchaseOrders` payload + response |
| `exact-rate-limit.interface.ts` | Rate limit header parsing types |

### Frontend (Next.js)

| Page/Component | Purpose |
|----------------|---------|
| `/admin/exact` | Connection management: enter API key + division, test, connect/disconnect |
| `/admin/exact/sync` | Bulk sync controls: sync relations, sync articles, progress bars, error list |
| Order detail Exact section | Sync status badge, "Push to Exact" button, SO/PO status table |
| PO detail Exact section | Individual PO sync status, retry button |

---

## 2. Data Flow Diagrams

### 2a. Connection Setup Flow

```
Admin enters API key + division ID
        |
        v
POST /api/exact/connect { apiKey, divisionId }
        |
        v
exact-auth.service: AES-256-GCM encrypt(apiKey)
        |
        v
Deactivate existing active connection
        |
        v
INSERT exact_connections (api_key_enc, encryption_iv, division_id, is_active=true)
        |
        v
Verify: GET /api/v1/{division}/current/Me with Authorization: Bearer {apiKey}
        |
        v
Success → connection active | Failure → rollback, show error
```

### 2b. Bulk Sync Flow (Relations)

```
Admin clicks "Sync Relations"
        |
        v
Query: relations WHERE exact_account_guid IS NULL
  AND is_active = true
  AND (exact_supplier_id IS NOT NULL OR exact_customer_id IS NOT NULL)
        |
        v
For each relation (~200):
  ├── Pre-flight validate (name, code present)
  ├── POST /crm/Accounts { Code, Name, IsPurchase, IsSales, ... }
  ├── Response: { ID: "guid-here" }
  ├── UPDATE relations SET exact_account_guid = ID, exact_synced_at = now()
  └── INSERT exact_sync_log (entity_type='account', status='success')
        |
        v
Return SyncProgress { total: 200, synced: 195, failed: 5, errors: [...] }
```

### 2c. Bulk Sync Flow (Articles — with deduplication)

```
Admin clicks "Sync Articles"
        |
        v
Query: articles WHERE exact_item_guid IS NULL
  AND exact_item_code IS NOT NULL AND is_active = true
        |
        v
Group by exact_item_code → Map<code, Article[]>
  (200k articles → ~Nk unique codes after dedup)
        |
        v
For each unique code:
  ├── Pre-flight validate (code, description present)
  ├── POST /logistics/Items { Code, Description, IsSalesItem, IsPurchaseItem, ... }
  ├── Response: { ID: "guid-here" }
  ├── UPDATE articles SET exact_item_guid = ID WHERE exact_item_code = code
  │   (updates ALL articles sharing this code)
  └── INSERT exact_sync_log (entity_type='item', status='success')
        |
        v
Circuit breaker: stop at 7 errors per endpoint (safety margin before 10-error block)
```

### 2d. Order Push Flow (Fan-out)

```
User clicks "Push to Exact" on Order 700048
        |
        v
Dependency check:
  ├── Customer: relations.exact_account_guid for order.customer_id  → EXISTS?
  ├── Suppliers: relations.exact_account_guid for each unique supplier → ALL EXIST?
  └── Articles: articles.exact_item_guid for each order line article  → ALL EXIST?
        |
        ├── Missing deps → Return checklist of what needs sync first
        |
        v (all deps present)
1. POST /salesorder/SalesOrders
   {
     OrderedBy: customer.exact_account_guid,
     SalesOrderLines: [
       { Item: article.exact_item_guid, Quantity: qty, NetPrice: unit_sell_price }
       ...for each order line
     ]
   }
   → Store exact_sales_order_id + exact_sales_order_number on orders
   → Store exact_sales_line_id on each order_line
        |
        v
2. For each unique supplier on the order lines:
   POST /purchaseorder/PurchaseOrders
   {
     Supplier: supplier.exact_account_guid,
     PurchaseOrderLines: [
       { Item: article.exact_item_guid, QuantityInPurchaseUnits: qty, NetPrice: unit_purchase_price }
       ...for lines of this supplier
     ]
   }
   → Store exact_purchase_order_id + exact_purchase_order_number on purchase_orders
   → Store exact_po_line_id on each purchase_order_line
        |
        v
3. UPDATE orders SET exact_sync_status = 'synced', exact_last_synced_at = now()
   UPDATE purchase_orders SET exact_sync_status = 'synced', exact_last_synced_at = now()
   INSERT exact_sync_log entries for SO + each PO
        |
        v
Partial failure handling:
  If SO succeeds but PO(s) fail → status = 'sync_failed'
  Log which POs failed, allow individual retry
```

---

## 3. Exact Online API Contracts

### 3a. Create Account (Relation)

```
POST /api/v1/{division}/crm/Accounts
Authorization: Bearer {API_KEY}
Content-Type: application/json

{
  "Code": "1234",                    // from exact_supplier_id or exact_customer_id
  "Name": "Boltex Marine",           // from relations.name (max 50 chars)
  "IsPurchase": true,                // from relations.is_supplier
  "IsSales": true,                   // from relations.is_customer
  "Country": "NL",                   // from relations.country (ISO2 — VERIFY FORMAT)
  "City": "Rotterdam",
  "Postcode": "3011 AA",
  "Phone": "+31 10 1234567",
  "Email": "info@boltex.nl",
  "VATNumber": "NL123456789B01"
}

Response: { "d": { "ID": "3fa85f64-...", "Code": "1234", ... } }
Store: ID → relations.exact_account_guid
```

### 3b. Create Item (Article)

```
POST /api/v1/{division}/logistics/Items
Authorization: Bearer {API_KEY}

{
  "Code": "WSY-1000001",            // from articles.exact_item_code
  "Description": "SS BOLT M12X80",  // from articles.description (max 60 chars)
  "IsSalesItem": true,
  "IsPurchaseItem": true,
  "Unit": "pc",                      // from articles.unit (VERIFY Exact unit codes)
  "Barcode": "8710398012345",        // from articles.ean_code (optional)
  "NetWeight": 0.125                 // from articles.weight_per_item_kg (optional)
}

Response: { "d": { "ID": "guid-here", "Code": "WSY-1000001", ... } }
Store: ID → articles.exact_item_guid (for ALL articles with same exact_item_code)
```

### 3c. Create Sales Order

```
POST /api/v1/{division}/salesorder/SalesOrders
Authorization: Bearer {API_KEY}

{
  "OrderedBy": "customer-account-guid",       // from relations.exact_account_guid
  "YourRef": "700048",                         // from orders.order_number
  "Description": "Order 700048 - MY SERENITY",
  "SalesOrderLines": [
    {
      "Item": "article-item-guid",             // from articles.exact_item_guid
      "Quantity": 500.000,                     // from order_lines.quantity
      "NetPrice": 1.67,                        // from order_lines.unit_sell_price
      "Description": "SS Bolt M12x80"          // from order_lines.description_snapshot
    }
  ]
}

Response: { "d": { "OrderID": "guid", "OrderNumber": 12345, "SalesOrderLines": { "results": [...] } } }
Store: OrderID → orders.exact_sales_order_id
       OrderNumber → orders.exact_sales_order_number
       Per line ID → order_lines.exact_sales_line_id
```

### 3d. Create Purchase Order

```
POST /api/v1/{division}/purchaseorder/PurchaseOrders
Authorization: Bearer {API_KEY}

{
  "Supplier": "supplier-account-guid",         // from relations.exact_account_guid
  "YourRef": "PO-800001",                      // from purchase_orders.po_number
  "ReceiptDate": "2026-04-15",                 // from orders.load_date
  "PurchaseOrderLines": [
    {
      "Item": "article-item-guid",             // from articles.exact_item_guid
      "QuantityInPurchaseUnits": 500.000,      // from purchase_order_lines.quantity
      "NetPrice": 1.45,                        // from purchase_order_lines.unit_purchase_price
      "Description": "SS Bolt M12x80"          // from purchase_order_lines.description_snapshot
    }
  ]
}

Response: { "d": { "PurchaseOrderID": "guid", "PurchaseOrderNumber": 67890, ... } }
Store: PurchaseOrderID → purchase_orders.exact_purchase_order_id
       PurchaseOrderNumber → purchase_orders.exact_purchase_order_number
       Per line ID → purchase_order_lines.exact_po_line_id
```

---

## 4. Dependency Chain

Strict ordering — each level requires the previous to be complete:

```
Level 0: exact_connections
  │  Admin connects SUPWISE to Exact with API key + division ID
  │
Level 1: Relations → Exact Accounts
  │  ~200 relations → POST /crm/Accounts → store exact_account_guid
  │  Covers both customers (OrderedBy) and suppliers (Supplier)
  │
Level 2: Articles → Exact Items
  │  ~Nk unique exact_item_code values → POST /logistics/Items → store exact_item_guid
  │  Deduplication: many SUPWISE articles share one Exact item code
  │
Level 3: Orders → Exact Sales Orders + Purchase Orders
     Per order: check all GUIDs present, then:
     3a. POST Sales Order (needs customer GUID + all article GUIDs)
     3b. POST Purchase Orders (needs supplier GUIDs + article GUIDs)
```

**Why this order matters:**
- Sales Orders require `OrderedBy` (customer GUID) and `Item` (article GUID) per line
- Purchase Orders require `Supplier` (supplier GUID) and `Item` (article GUID) per line
- If any GUID is missing, the Exact API returns 400 → counts toward error velocity limit
- Pre-flight validation checks all GUIDs exist before making any API call

---

## 5. Database Schema Changes

### 5a. New Table: `exact_connections`

```sql
CREATE TABLE exact_connections (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  division_id     INT NOT NULL,
  api_key_enc     TEXT NOT NULL,                    -- AES-256-GCM encrypted API key
  encryption_iv   TEXT NOT NULL,                    -- IV for decryption
  is_active       BOOLEAN NOT NULL DEFAULT true,
  connected_by    UUID NOT NULL REFERENCES user_profiles(id),
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Only 1 active connection at a time
CREATE UNIQUE INDEX uq_exact_connections_active
  ON exact_connections (is_active) WHERE is_active = true;
```

**Key difference from PRD:** No `access_token_enc`, `refresh_token_enc`, or `token_expires_at`. API key does not expire, has no refresh cycle.

### 5b. New Columns on `relations`

```sql
ALTER TABLE relations
  ADD COLUMN exact_account_guid UUID,          -- Exact Account ID (returned by POST)
  ADD COLUMN exact_synced_at TIMESTAMPTZ;      -- Last successful sync timestamp

CREATE INDEX idx_relations_exact_account_guid
  ON relations (exact_account_guid) WHERE exact_account_guid IS NOT NULL;
```

**Existing columns preserved:** `exact_supplier_id` (TEXT) and `exact_customer_id` (TEXT) remain as reference codes from CSV import. The new `exact_account_guid` stores the actual Exact Online GUID.

### 5c. New Columns on `articles`

```sql
ALTER TABLE articles
  ADD COLUMN exact_item_guid UUID,             -- Exact Item ID (returned by POST)
  ADD COLUMN exact_synced_at TIMESTAMPTZ;

CREATE INDEX idx_articles_exact_item_guid
  ON articles (exact_item_guid) WHERE exact_item_guid IS NOT NULL;

-- Index for deduplication: quickly find all articles sharing same exact_item_code
CREATE INDEX idx_articles_exact_item_code_dedup
  ON articles (exact_item_code) WHERE exact_item_code IS NOT NULL;
```

**Existing column preserved:** `exact_item_code` (TEXT, not unique) remains as the Exact item code reference. The new `exact_item_guid` stores the GUID.

### 5d. New Columns on `orders`

```sql
ALTER TABLE orders
  ADD COLUMN exact_sales_order_id    UUID,     -- Exact Sales Order ID
  ADD COLUMN exact_sales_order_number INT,     -- Exact human-readable number
  ADD COLUMN exact_sync_status       TEXT NOT NULL DEFAULT 'not_synced'
    CHECK (exact_sync_status IN ('not_synced', 'synced', 'sync_failed', 'outdated')),
  ADD COLUMN exact_last_synced_at    TIMESTAMPTZ;
```

### 5e. New Column on `order_lines`

```sql
ALTER TABLE order_lines
  ADD COLUMN exact_sales_line_id UUID;         -- Exact SalesOrderLine ID
```

### 5f. New Columns on `purchase_orders`

```sql
ALTER TABLE purchase_orders
  ADD COLUMN exact_purchase_order_id     UUID,
  ADD COLUMN exact_purchase_order_number INT,
  ADD COLUMN exact_sync_status           TEXT NOT NULL DEFAULT 'not_synced'
    CHECK (exact_sync_status IN ('not_synced', 'synced', 'sync_failed')),
  ADD COLUMN exact_last_synced_at        TIMESTAMPTZ;
```

### 5g. New Column on `purchase_order_lines`

```sql
ALTER TABLE purchase_order_lines
  ADD COLUMN exact_po_line_id UUID;            -- Exact PurchaseOrderLine ID
```

### 5h. New Table: `exact_sync_log`

```sql
CREATE TABLE exact_sync_log (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_type     TEXT NOT NULL
    CHECK (entity_type IN ('account', 'item', 'sales_order', 'purchase_order')),
  entity_id       UUID NOT NULL,
  action          TEXT NOT NULL
    CHECK (action IN ('create', 'update', 'delete')),
  status          TEXT NOT NULL
    CHECK (status IN ('success', 'failed', 'retrying')),
  attempt         INT NOT NULL DEFAULT 1,
  error_message   TEXT,
  request_body    JSONB,
  response_body   JSONB,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_by      UUID REFERENCES user_profiles(id)
);

CREATE INDEX idx_exact_sync_log_entity ON exact_sync_log (entity_type, entity_id);
CREATE INDEX idx_exact_sync_log_status ON exact_sync_log (status) WHERE status = 'failed';
CREATE INDEX idx_exact_sync_log_created ON exact_sync_log (created_at DESC);
```

### 5i. Trigger: Outdated Detection

```sql
CREATE OR REPLACE FUNCTION mark_order_exact_outdated()
RETURNS TRIGGER AS $$
BEGIN
  IF OLD.exact_sync_status = 'synced' THEN
    NEW.exact_sync_status := 'outdated';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_order_exact_outdated
  BEFORE UPDATE OF description, reference, load_date, customer_id,
    default_margin_pct, currency, delivery_method
  ON orders
  FOR EACH ROW
  WHEN (OLD.exact_sync_status = 'synced')
  EXECUTE FUNCTION mark_order_exact_outdated();
```

### 5j. Prisma Schema Additions

```prisma
// On Relation model:
exactAccountGuid  String?   @map("exact_account_guid") @db.Uuid
exactSyncedAt     DateTime? @map("exact_synced_at") @db.Timestamptz

// On Article model:
exactItemGuid     String?   @map("exact_item_guid") @db.Uuid
exactSyncedAt     DateTime? @map("exact_synced_at") @db.Timestamptz

// On Order model:
exactSalesOrderId     String?   @map("exact_sales_order_id") @db.Uuid
exactSalesOrderNumber Int?      @map("exact_sales_order_number")
exactSyncStatus       String    @default("not_synced") @map("exact_sync_status")
exactLastSyncedAt     DateTime? @map("exact_last_synced_at") @db.Timestamptz

// On OrderLine model:
exactSalesLineId  String?   @map("exact_sales_line_id") @db.Uuid

// On PurchaseOrder model:
exactPurchaseOrderId     String?   @map("exact_purchase_order_id") @db.Uuid
exactPurchaseOrderNumber Int?      @map("exact_purchase_order_number")
exactSyncStatus          String    @default("not_synced") @map("exact_sync_status")
exactLastSyncedAt        DateTime? @map("exact_last_synced_at") @db.Timestamptz

// On PurchaseOrderLine model:
exactPoLineId  String?   @map("exact_po_line_id") @db.Uuid

// New model:
model ExactConnection {
  id            String   @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  divisionId    Int      @map("division_id")
  apiKeyEnc     String   @map("api_key_enc")
  encryptionIv  String   @map("encryption_iv")
  isActive      Boolean  @default(true) @map("is_active")
  connectedBy   String   @map("connected_by") @db.Uuid
  createdAt     DateTime @default(now()) @map("created_at") @db.Timestamptz
  updatedAt     DateTime @updatedAt @map("updated_at") @db.Timestamptz

  connectedByUser UserProfile @relation(fields: [connectedBy], references: [id])

  @@map("exact_connections")
}

model ExactSyncLog {
  id           String   @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  entityType   String   @map("entity_type")
  entityId     String   @map("entity_id") @db.Uuid
  action       String
  status       String
  attempt      Int      @default(1)
  errorMessage String?  @map("error_message")
  requestBody  Json?    @map("request_body")
  responseBody Json?    @map("response_body")
  createdAt    DateTime @default(now()) @map("created_at") @db.Timestamptz
  createdBy    String?  @map("created_by") @db.Uuid

  creator UserProfile? @relation(fields: [createdBy], references: [id])

  @@index([entityType, entityId])
  @@map("exact_sync_log")
}
```

---

## 6. Implementation Sequence

### Sprint 1: Foundation (Days 1-3)

| # | Task | Deliverable |
|---|------|-------------|
| 1.1 | Supabase migration: `exact_connections` table | SQL migration file |
| 1.2 | Supabase migration: new columns on relations, articles, orders, POs, lines | SQL migration file |
| 1.3 | Supabase migration: `exact_sync_log` table + outdated trigger | SQL migration file |
| 1.4 | Prisma schema: add all new fields + models | `schema.prisma` update |
| 1.5 | `prisma generate` + verify types | Generated client |
| 1.6 | `ExactModule` scaffold: module, controller stub, all service stubs | Module files |
| 1.7 | `ExactAuthService`: encrypt, decrypt, connect, disconnect | Working auth service |
| 1.8 | Config: `EXACT_ENCRYPTION_KEY` env var + validation | Config schema update |
| 1.9 | Admin page: connect/disconnect form | Frontend page |

### Sprint 2: API Client + Rate Limiting (Days 4-6)

| # | Task | Deliverable |
|---|------|-------------|
| 2.1 | `ExactApiService`: HTTP client with Bearer auth, retry logic | Working API client |
| 2.2 | `ExactRateLimiterService`: dual header monitoring (minutely + daily) | Rate limiter |
| 2.3 | Error velocity tracking: per-endpoint error counter | Circuit breaker |
| 2.4 | `ExactValidationService`: pre-flight payload validation | Validation service |
| 2.5 | Connection verification: `GET /current/Me` on connect | Verify endpoint |
| 2.6 | `ExactSyncLogService`: CRUD for sync log | Log service |

### Sprint 3: Bulk Sync (Days 7-9)

| # | Task | Deliverable |
|---|------|-------------|
| 3.1 | `syncAllRelations()`: query unsynchronized, POST accounts, store GUIDs | Relations sync |
| 3.2 | `syncAllArticles()`: dedup by exact_item_code, POST items, share GUIDs | Articles sync |
| 3.3 | Sync progress endpoint: SSE or polling for progress updates | Progress API |
| 3.4 | Admin sync page: trigger sync, progress bars, error list | Frontend sync UI |
| 3.5 | Trial run: push 5 test records (2 relations, 3 articles) first | Trial run logic |

### Sprint 4: Order Push (Days 10-12)

| # | Task | Deliverable |
|---|------|-------------|
| 4.1 | Dependency checker: verify all GUIDs exist for an order | Dependency check |
| 4.2 | `pushOrder()`: build + send Sales Order | SO push |
| 4.3 | `pushOrder()`: build + send Purchase Orders per supplier | PO push |
| 4.4 | Partial failure handling: SO ok but PO fails | Error recovery |
| 4.5 | Order detail Exact section: status badges, push button | Frontend UI |
| 4.6 | PO detail Exact section: status badge, retry button | Frontend UI |
| 4.7 | Sync status badges on order overview list | Badge column |

### Sprint 5: Polish + Testing (Days 13-14)

| # | Task | Deliverable |
|---|------|-------------|
| 5.1 | Outdated detection: trigger test (edit order after sync) | Trigger works |
| 5.2 | Sync log viewer: admin page showing all sync attempts | Log UI |
| 5.3 | Error scenarios: 429, 400, timeout, partial failure | Error handling verified |
| 5.4 | Integration test with Exact test division | E2E test |
| 5.5 | Documentation: env vars, setup guide | Internal docs |

**Total: ~14 working days** (2.5 weeks + buffer for rate limit issues during bulk sync)

---

## 7. Risk Assessment

### Critical Risks

| # | Risk | Severity | Probability | Mitigation |
|---|------|----------|-------------|------------|
| R1 | **Bulk article sync duration** — 200k articles at 5,000/day = 40 workdays (even after dedup to 50k unique codes = 10 days) | CRITICAL | HIGH | Deduplication on `exact_item_code` is essential. Ask Exact about higher initial import limits. Worst case: background job running over multiple days with resume capability. |
| R2 | **Error velocity limit** — 10 errors per endpoint per hour = 1 hour block | HIGH | MEDIUM | Circuit breaker stops at 7 errors. Pre-flight validation catches bad payloads before they reach the API. Trial run of 5 records before bulk sync. |
| R3 | **API key auth header format unverified** — docs describe OAuth2, API key format may be `Bearer {key}` or `ApiKey {key}` or custom header | MEDIUM | LOW | First action on connect: test `GET /current/Me` with the provided API key. If format is wrong, fail fast with clear error. |

### Medium Risks

| # | Risk | Mitigation |
|---|------|------------|
| R4 | Mandatory `$filter` on GET endpoints | All lookup queries include explicit filters. Use Sync API (`/sync/...`) for bulk reads (1000 records/call). |
| R5 | N+1 query problem (no `$expand` support) | Minimize GET calls. For order push, we only POST — no reads needed. For status checks (v2), use Sync API. |
| R6 | JSON error `code` field is empty in Exact responses | Parse `message.value` string for error classification. Map known strings to error types. |
| R7 | Country code format unknown (ISO2? ISO3? Full name?) | Verify with test call. Build mapping table if needed. |
| R8 | Exact unit codes may differ from SUPWISE units (pc, kg, ltr) | Verify unit mapping with test call. Build mapping if needed. |

### Low Risks

| # | Risk | Mitigation |
|---|------|------------|
| R9 | Test division may have pre-existing data causing conflicts | Use `$filter` to check for existing items by Code before POST. Make sync idempotent. |
| R10 | API key revocation without notification | Connection test endpoint called periodically (or on first error). Admin notified if key is invalid. |

---

## 8. API Endpoints (SUPWISE Backend)

| Method | Path | Purpose | Auth |
|--------|------|---------|------|
| POST | `/api/exact/connect` | Store encrypted API key + division, verify connection | Admin only |
| DELETE | `/api/exact/disconnect` | Deactivate current connection | Admin only |
| GET | `/api/exact/status` | Return connection status + last sync times | Admin only |
| POST | `/api/exact/sync/relations` | Start bulk sync of relations to Exact Accounts | Admin only |
| POST | `/api/exact/sync/articles` | Start bulk sync of articles to Exact Items | Admin only |
| GET | `/api/exact/sync/progress` | Get current sync progress (SSE or polling) | Admin only |
| POST | `/api/exact/push/order/:id` | Push one order (SO + POs) to Exact | Admin, OB |
| POST | `/api/exact/push/purchase-order/:id` | Retry push for one failed PO | Admin, OB |
| GET | `/api/exact/sync-log` | Query sync log entries (paginated, filterable) | Admin only |
| GET | `/api/exact/order/:id/deps` | Check dependency status for an order | Admin, OB |

---

## 9. Environment Configuration

```env
# Phase 5B — Exact Online
EXACT_ENCRYPTION_KEY=          # 32-byte hex key (64 hex chars) for AES-256-GCM encryption
```

**Config validation (Joi):**
```typescript
EXACT_ENCRYPTION_KEY: Joi.string().hex().length(64).optional()
```

The encryption key is used only for encrypting/decrypting the API key at rest in `exact_connections`. It must be generated once and stored securely. If lost, the admin must re-enter the API key.

---

## 10. Deviations from Original PRD (`exact-koppeling.md`)

| Topic | Original PRD | This Plan | Reason |
|-------|-------------|-----------|--------|
| Auth mechanism | OAuth2 Authorization Code flow | API key (Bearer token) | Confirmed by Exact Online meeting. Private app registration, no OAuth flow needed. |
| Token management | access_token + refresh_token, 10min expiry, auto-refresh | Single API key, no expiry, no refresh | API key is simpler, no token lifecycle management |
| `exact_connections` table | Has `access_token_enc`, `refresh_token_enc`, `token_expires_at` | Has `api_key_enc`, `encryption_iv` only | No tokens to manage |
| GUID storage | Overwrite existing `exact_supplier_id`/`exact_customer_id` TEXT fields | New `exact_account_guid` UUID column alongside existing TEXT codes | Keep CSV import codes as reference, add clean GUID column |
| Error handling | Retry 3x | Three-layer: pre-flight validation + circuit breaker (7 errors) + retry 3x for transient errors | Error velocity limit (10/hour/endpoint) makes blind retry dangerous |
| Rate limiting | Simple counter | Dual header monitoring (minutely + daily) + error velocity counter | Exact switches between daily and minutely headers — must monitor both |
