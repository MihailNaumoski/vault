# Phase 5B: Exact Online Integration -- Detailed Implementation Plan

**Date:** 2026-04-07
**Author:** Backend Dev (Engineering Team)
**Based on:** Full codebase analysis of SUPWISE + Exact Online API research + user-confirmed API key auth

---

## 1. Module Structure

```
apps/api/src/exact/
├── exact.module.ts                 # NestJS module: imports, providers, exports
├── exact.controller.ts             # Admin endpoints: connect, disconnect, status
├── exact-sync.controller.ts        # Sync endpoints: start, progress, pause
├── exact-push.controller.ts        # Push endpoints: push order, retry, preview
├── exact-auth.service.ts           # API key encrypt/decrypt (AES-256-GCM), connection CRUD
├── exact-api.service.ts            # HTTP client: dual rate limiting, retry, circuit breaker
├── exact-validation.service.ts     # Pre-flight validation before each Exact API call
├── exact-sync.service.ts           # Bulk sync: relations -> Accounts, articles -> Items
├── exact-push.service.ts           # Order push: SO + POs, dependency checking, partial failure
├── exact-sync-log.service.ts       # Sync log CRUD + query
├── dto/
│   ├── connect-exact.dto.ts        # { apiKey: string, divisionId: number }
│   ├── disconnect-exact.dto.ts     # (empty or confirmation)
│   ├── sync-entities.dto.ts        # { resume?: boolean, batchSize?: number }
│   ├── push-order.dto.ts           # { skipValidation?: boolean }
│   ├── retry-push.dto.ts           # { purchaseOrderIds?: string[] }
│   ├── list-sync-log.dto.ts        # { entityType?, status?, limit?, cursor? }
│   └── index.ts                    # Barrel export
└── interfaces/
    ├── exact-account.interface.ts   # Exact Account payload/response types
    ├── exact-item.interface.ts      # Exact Item payload/response types
    ├── exact-sales-order.interface.ts
    ├── exact-purchase-order.interface.ts
    ├── exact-rate-limit.interface.ts
    ├── sync-progress.interface.ts   # { total, synced, failed, errors[], jobId }
    └── index.ts
```

### Module Wiring: `exact.module.ts`

```typescript
import { Module } from '@nestjs/common';
import { HttpModule } from '@nestjs/axios';
import { DatabaseModule } from '../database/database.module';
import { ExactController } from './exact.controller';
import { ExactSyncController } from './exact-sync.controller';
import { ExactPushController } from './exact-push.controller';
import { ExactAuthService } from './exact-auth.service';
import { ExactApiService } from './exact-api.service';
import { ExactValidationService } from './exact-validation.service';
import { ExactSyncService } from './exact-sync.service';
import { ExactPushService } from './exact-push.service';
import { ExactSyncLogService } from './exact-sync-log.service';

@Module({
  imports: [
    DatabaseModule,
    HttpModule.register({
      timeout: 30_000,
      maxRedirects: 0,
    }),
  ],
  controllers: [ExactController, ExactSyncController, ExactPushController],
  providers: [
    ExactAuthService,
    ExactApiService,
    ExactValidationService,
    ExactSyncService,
    ExactPushService,
    ExactSyncLogService,
  ],
  exports: [ExactAuthService, ExactPushService], // Export for potential use by OrdersModule
})
export class ExactModule {}
```

**Registration in `app.module.ts`** (line ~24, after existing imports):

```typescript
import { ExactModule } from './exact/exact.module';

// In imports array (line ~89, after DashboardModule):
ExactModule,
```

This follows the exact same pattern as all other modules in the existing `app.module.ts` (lines 13-28 for imports, lines 75-89 for registration).

---

## 2. Database Migration

### Migration file: `supabase/migrations/YYYYMMDDHHMMSS_phase5b_exact_koppeling.sql`

```sql
-- ============================================================================
-- Phase 5B: Exact Online Koppeling
-- Migration: phase5b_exact_koppeling
-- ============================================================================

-- ==========================================================================
-- 1. exact_connections -- API key storage (NOT OAuth tokens)
-- ==========================================================================
CREATE TABLE exact_connections (
  id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  division_id     INT NOT NULL,
  api_key_enc     TEXT NOT NULL,                    -- AES-256-GCM encrypted API key
  encryption_iv   TEXT NOT NULL,                    -- IV for decryption (hex-encoded)
  is_active       BOOLEAN NOT NULL DEFAULT true,
  daily_calls_used INT NOT NULL DEFAULT 0,          -- Persisted daily counter
  daily_reset_at  TIMESTAMPTZ,                      -- When daily counter resets
  last_error_at   TIMESTAMPTZ,                      -- Last API error timestamp
  connected_by    UUID NOT NULL REFERENCES user_profiles(id),
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Only one active connection at a time
CREATE UNIQUE INDEX uq_exact_connections_active
  ON exact_connections (is_active) WHERE is_active = true;

COMMENT ON TABLE exact_connections IS 'Stores encrypted Exact Online API key. Only one active connection allowed.';

-- RLS: service role only (API key must never be exposed to client)
ALTER TABLE exact_connections ENABLE ROW LEVEL SECURITY;

CREATE POLICY "Service role full access on exact_connections"
  ON exact_connections FOR ALL
  USING (true) WITH CHECK (true);

-- ==========================================================================
-- 2. GUID columns on relations (Exact Accounts)
-- ==========================================================================
ALTER TABLE relations
  ADD COLUMN exact_account_guid UUID,
  ADD COLUMN exact_synced_at TIMESTAMPTZ;

CREATE INDEX idx_relations_exact_account_guid
  ON relations (exact_account_guid) WHERE exact_account_guid IS NOT NULL;

COMMENT ON COLUMN relations.exact_account_guid IS 'Exact Online Account GUID, set after POST to crm/Accounts';

-- ==========================================================================
-- 3. GUID column on articles (Exact Items)
-- ==========================================================================
ALTER TABLE articles
  ADD COLUMN exact_item_guid UUID,
  ADD COLUMN exact_synced_at TIMESTAMPTZ;

CREATE INDEX idx_articles_exact_item_guid
  ON articles (exact_item_guid) WHERE exact_item_guid IS NOT NULL;

-- Deduplication index: quickly find all articles sharing the same exact_item_code
CREATE INDEX idx_articles_exact_item_code_dedup
  ON articles (exact_item_code) WHERE exact_item_code IS NOT NULL;

COMMENT ON COLUMN articles.exact_item_guid IS 'Exact Online Item GUID, set after POST to logistics/Items';

-- ==========================================================================
-- 4. Exact fields on orders (Sales Orders)
-- ==========================================================================
ALTER TABLE orders
  ADD COLUMN exact_sales_order_id    UUID,
  ADD COLUMN exact_sales_order_number INT,
  ADD COLUMN exact_sync_status       TEXT NOT NULL DEFAULT 'not_synced'
    CHECK (exact_sync_status IN ('not_synced', 'synced', 'sync_failed', 'outdated')),
  ADD COLUMN exact_last_synced_at    TIMESTAMPTZ;

CREATE INDEX idx_orders_exact_sync_status
  ON orders (exact_sync_status) WHERE exact_sync_status != 'not_synced';

-- ==========================================================================
-- 5. Exact fields on purchase_orders
-- ==========================================================================
ALTER TABLE purchase_orders
  ADD COLUMN exact_purchase_order_id     UUID,
  ADD COLUMN exact_purchase_order_number INT,
  ADD COLUMN exact_sync_status           TEXT NOT NULL DEFAULT 'not_synced'
    CHECK (exact_sync_status IN ('not_synced', 'synced', 'sync_failed')),
  ADD COLUMN exact_last_synced_at        TIMESTAMPTZ;

-- ==========================================================================
-- 6. Exact line-level GUIDs on order_lines
-- ==========================================================================
ALTER TABLE order_lines
  ADD COLUMN exact_sales_line_id UUID;

-- ==========================================================================
-- 7. Exact line-level GUIDs on purchase_order_lines
-- ==========================================================================
ALTER TABLE purchase_order_lines
  ADD COLUMN exact_purchase_line_id UUID;

-- ==========================================================================
-- 8. exact_sync_log -- Audit trail of all sync/push attempts
-- ==========================================================================
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
  error_code      TEXT,                             -- HTTP status code from Exact
  request_body    JSONB,                            -- What was sent (secrets stripped)
  response_body   JSONB,                            -- What Exact returned
  duration_ms     INT,                              -- How long the API call took
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  created_by      UUID REFERENCES user_profiles(id)
);

CREATE INDEX idx_exact_sync_log_entity
  ON exact_sync_log (entity_type, entity_id);
CREATE INDEX idx_exact_sync_log_status
  ON exact_sync_log (status) WHERE status = 'failed';
CREATE INDEX idx_exact_sync_log_created
  ON exact_sync_log (created_at DESC);
CREATE INDEX idx_exact_sync_log_entity_type_created
  ON exact_sync_log (entity_type, created_at DESC);

ALTER TABLE exact_sync_log ENABLE ROW LEVEL SECURITY;

CREATE POLICY "Service role full access on exact_sync_log"
  ON exact_sync_log FOR ALL
  USING (true) WITH CHECK (true);

-- ==========================================================================
-- 9. Trigger: mark order as 'outdated' when modified after sync
-- ==========================================================================
-- Fires when key order fields change AFTER the order was already synced to Exact.
-- This lets the UI show a "re-sync needed" badge.
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

-- Also detect line changes by adding a trigger on order_lines
-- that marks the parent order as outdated
CREATE OR REPLACE FUNCTION mark_parent_order_exact_outdated()
RETURNS TRIGGER AS $$
BEGIN
  UPDATE orders
  SET exact_sync_status = 'outdated'
  WHERE id = COALESCE(NEW.order_id, OLD.order_id)
    AND exact_sync_status = 'synced';
  RETURN COALESCE(NEW, OLD);
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_order_line_exact_outdated
  AFTER INSERT OR UPDATE OR DELETE
  ON order_lines
  FOR EACH ROW
  EXECUTE FUNCTION mark_parent_order_exact_outdated();

-- ==========================================================================
-- 10. updated_at auto-trigger for exact_connections
-- ==========================================================================
CREATE OR REPLACE FUNCTION update_exact_connections_updated_at()
RETURNS TRIGGER AS $$
BEGIN
  NEW.updated_at := now();
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_exact_connections_updated_at
  BEFORE UPDATE ON exact_connections
  FOR EACH ROW
  EXECUTE FUNCTION update_exact_connections_updated_at();
```

---

## 3. Prisma Schema Additions

Add these to `packages/database/prisma/schema.prisma`, following the existing `@@map`/`@map` conventions:

### New model: `ExactConnection`

```prisma
// After AuditLog model (line ~991)

// ============================================================================
// MODULE: Exact Online Integration (Phase 5B)
// ============================================================================

model ExactConnection {
  id              String    @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  divisionId      Int       @map("division_id")
  apiKeyEnc       String    @map("api_key_enc")
  encryptionIv    String    @map("encryption_iv")
  isActive        Boolean   @default(true) @map("is_active")
  dailyCallsUsed  Int       @default(0) @map("daily_calls_used")
  dailyResetAt    DateTime? @map("daily_reset_at") @db.Timestamptz
  lastErrorAt     DateTime? @map("last_error_at") @db.Timestamptz
  connectedBy     String    @map("connected_by") @db.Uuid
  createdAt       DateTime  @default(now()) @map("created_at") @db.Timestamptz
  updatedAt       DateTime  @updatedAt @map("updated_at") @db.Timestamptz

  connectedByUser UserProfile @relation(fields: [connectedBy], references: [id])

  @@map("exact_connections")
}
```

### New model: `ExactSyncLog`

```prisma
model ExactSyncLog {
  id            String   @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  entityType    String   @map("entity_type")
  entityId      String   @map("entity_id") @db.Uuid
  action        String
  status        String
  attempt       Int      @default(1)
  errorMessage  String?  @map("error_message")
  errorCode     String?  @map("error_code")
  requestBody   Json?    @map("request_body")
  responseBody  Json?    @map("response_body")
  durationMs    Int?     @map("duration_ms")
  createdAt     DateTime @default(now()) @map("created_at") @db.Timestamptz
  createdBy     String?  @map("created_by") @db.Uuid

  createdByUser UserProfile? @relation(fields: [createdBy], references: [id])

  @@index([entityType, entityId])
  @@index([status])
  @@index([createdAt(sort: Desc)])
  @@map("exact_sync_log")
}
```

### Fields on existing models

**On `Relation` model** (after `searchVector` field, line ~405):

```prisma
  exactAccountGuid  String?   @map("exact_account_guid") @db.Uuid
  exactSyncedAt     DateTime? @map("exact_synced_at") @db.Timestamptz
```

**On `Article` model** (after `searchVector` field, line ~298):

```prisma
  exactItemGuid     String?   @map("exact_item_guid") @db.Uuid
  exactSyncedAt     DateTime? @map("exact_synced_at") @db.Timestamptz
```

**On `Order` model** (after `packingNotes` field, line ~638):

```prisma
  exactSalesOrderId     String?   @map("exact_sales_order_id") @db.Uuid
  exactSalesOrderNumber Int?      @map("exact_sales_order_number")
  exactSyncStatus       String    @default("not_synced") @map("exact_sync_status")
  exactLastSyncedAt     DateTime? @map("exact_last_synced_at") @db.Timestamptz
```

**On `PurchaseOrder` model** (after `receivedAt` field, line ~770):

```prisma
  exactPurchaseOrderId     String?   @map("exact_purchase_order_id") @db.Uuid
  exactPurchaseOrderNumber Int?      @map("exact_purchase_order_number")
  exactSyncStatus          String    @default("not_synced") @map("exact_sync_status")
  exactLastSyncedAt        DateTime? @map("exact_last_synced_at") @db.Timestamptz
```

**On `OrderLine` model** (after `topMatches` field, line ~700):

```prisma
  exactSalesLineId  String?   @map("exact_sales_line_id") @db.Uuid
```

**On `PurchaseOrderLine` model** (after `sortOrder` field, line ~803):

```prisma
  exactPurchaseLineId  String?   @map("exact_purchase_line_id") @db.Uuid
```

**On `UserProfile` model** (after `receiveEvents` relation, line ~180):

```prisma
  exactConnections   ExactConnection[]
  exactSyncLogs      ExactSyncLog[]
```

---

## 4. Config Schema Additions

File: `apps/api/src/common/config/config.schema.ts` (currently 19 lines)

Add after `CORS_ORIGIN` (line 18):

```typescript
  // Exact Online (Phase 5B)
  EXACT_ENCRYPTION_KEY: Joi.string().hex().length(64).optional(),  // 32 bytes = 64 hex chars
  EXACT_BASE_URL: Joi.string().uri().default('https://start.exactonline.nl'),
```

**`.env.example` additions:**

```
# Exact Online (Phase 5B)
EXACT_ENCRYPTION_KEY=          # 32-byte hex key for AES-256-GCM API key encryption
EXACT_BASE_URL=                # Defaults to https://start.exactonline.nl
```

---

## 5. Service Implementations

### 5a. ExactAuthService -- API Key Management

```typescript
import { Injectable, UnauthorizedException, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import * as crypto from 'crypto';
import { DatabaseService } from '../database/database.service';

@Injectable()
export class ExactAuthService {
  private readonly logger = new Logger(ExactAuthService.name);

  constructor(
    private readonly db: DatabaseService,
    private readonly config: ConfigService,
  ) {}

  /**
   * Encrypt and store API key. Deactivates any existing connection first.
   * Follows same transaction pattern as OrdersService.create() (line 278-318).
   */
  async connect(apiKey: string, divisionId: number, userId: string) {
    const encryptionKey = this.getEncryptionKey();
    const iv = crypto.randomBytes(16);
    const cipher = crypto.createCipheriv('aes-256-gcm', encryptionKey, iv);

    let encrypted = cipher.update(apiKey, 'utf8', 'hex');
    encrypted += cipher.final('hex');
    const authTag = cipher.getAuthTag().toString('hex');

    // Deactivate existing + create new in a single transaction
    const connection = await this.db.$transaction(async (tx) => {
      await tx.exactConnection.updateMany({
        where: { isActive: true },
        data: { isActive: false },
      });

      return tx.exactConnection.create({
        data: {
          divisionId,
          apiKeyEnc: `${encrypted}:${authTag}`,
          encryptionIv: iv.toString('hex'),
          isActive: true,
          connectedBy: userId,
        },
      });
    });

    await this.db.logAudit(userId, 'CREATE', 'exact_connection', connection.id, {
      divisionId,
      // NEVER log the API key
    });

    return { id: connection.id, divisionId, isActive: true };
  }

  /**
   * Decrypt API key for use in API calls.
   * Returns both key and divisionId needed for URL construction.
   */
  async getApiKey(): Promise<{ apiKey: string; divisionId: number; connectionId: string }> {
    const conn = await this.db.exactConnection.findFirst({
      where: { isActive: true },
    });
    if (!conn) throw new UnauthorizedException('No active Exact connection');

    const encryptionKey = this.getEncryptionKey();
    const [encrypted, authTag] = conn.apiKeyEnc.split(':');
    const decipher = crypto.createDecipheriv(
      'aes-256-gcm',
      encryptionKey,
      Buffer.from(conn.encryptionIv, 'hex'),
    );
    decipher.setAuthTag(Buffer.from(authTag, 'hex'));

    let decrypted = decipher.update(encrypted, 'hex', 'utf8');
    decrypted += decipher.final('utf8');

    return { apiKey: decrypted, divisionId: conn.divisionId, connectionId: conn.id };
  }

  /**
   * Disconnect: deactivate connection (soft delete, keeps audit trail).
   */
  async disconnect(userId: string) {
    const conn = await this.db.exactConnection.findFirst({
      where: { isActive: true },
    });
    if (!conn) throw new UnauthorizedException('No active Exact connection');

    await this.db.exactConnection.update({
      where: { id: conn.id },
      data: { isActive: false },
    });

    await this.db.logAudit(userId, 'DELETE', 'exact_connection', conn.id, {
      divisionId: conn.divisionId,
    });

    return { disconnected: true };
  }

  /**
   * Get connection status (for admin UI). Never returns the API key.
   */
  async getStatus() {
    const conn = await this.db.exactConnection.findFirst({
      where: { isActive: true },
      select: {
        id: true,
        divisionId: true,
        isActive: true,
        dailyCallsUsed: true,
        dailyResetAt: true,
        lastErrorAt: true,
        createdAt: true,
        connectedByUser: { select: { id: true, fullName: true } },
      },
    });

    if (!conn) return { connected: false };

    // Count sync stats
    const [syncedRelations, syncedArticles, syncedOrders] = await Promise.all([
      this.db.relation.count({ where: { exactAccountGuid: { not: null } } }),
      this.db.article.count({ where: { exactItemGuid: { not: null } } }),
      this.db.order.count({ where: { exactSyncStatus: 'synced' } }),
    ]);

    return {
      connected: true,
      ...conn,
      stats: { syncedRelations, syncedArticles, syncedOrders },
    };
  }

  /**
   * Update daily call counter (called by ExactApiService after each request).
   */
  async incrementDailyCounter(connectionId: string) {
    await this.db.exactConnection.update({
      where: { id: connectionId },
      data: {
        dailyCallsUsed: { increment: 1 },
      },
    });
  }

  /**
   * Reset daily counter (called when X-RateLimit-Reset indicates new day).
   */
  async resetDailyCounter(connectionId: string) {
    await this.db.exactConnection.update({
      where: { id: connectionId },
      data: {
        dailyCallsUsed: 0,
        dailyResetAt: new Date(),
      },
    });
  }

  private getEncryptionKey(): Buffer {
    const keyHex = this.config.get<string>('EXACT_ENCRYPTION_KEY');
    if (!keyHex) throw new Error('EXACT_ENCRYPTION_KEY not configured');
    return Buffer.from(keyHex, 'hex');
  }
}
```

**Key design decisions:**
- AES-256-GCM with random IV per connection (authenticated encryption)
- Auth tag stored alongside ciphertext with `:` separator (same pattern used by Supabase vault)
- Daily counter persisted in DB so it survives server restarts
- Connection status endpoint returns stats without ever exposing the key

### 5b. ExactApiService -- HTTP Client with Dual Rate Limiting

```typescript
import { Injectable, Logger } from '@nestjs/common';
import { HttpService } from '@nestjs/axios';
import { ConfigService } from '@nestjs/config';
import { AxiosError, AxiosResponse } from 'axios';
import { ExactAuthService } from './exact-auth.service';

interface RateLimitState {
  minutelyCount: number;
  minutelyResetAt: number;      // epoch ms
  dailyRemaining: number;
  dailyResetAt: number;         // epoch ms
}

interface CircuitBreakerState {
  errorCounts: Map<string, { count: number; firstErrorAt: number }>;
  isOpen: boolean;
  openUntil: number;
}

@Injectable()
export class ExactApiService {
  private readonly logger = new Logger(ExactApiService.name);
  private readonly baseUrl: string;

  // In-memory rate limit state
  private rateLimit: RateLimitState = {
    minutelyCount: 0,
    minutelyResetAt: Date.now() + 60_000,
    dailyRemaining: 5000,
    dailyResetAt: 0,
  };

  // Circuit breaker: per-endpoint error tracking
  private circuitBreaker: CircuitBreakerState = {
    errorCounts: new Map(),
    isOpen: false,
    openUntil: 0,
  };

  // Soft limits (leave margin before hitting hard limits)
  private readonly SOFT_MINUTELY_LIMIT = 55;   // Hard: 60
  private readonly SOFT_DAILY_LIMIT = 4950;     // Hard: 5000
  private readonly MAX_ERRORS_PER_ENDPOINT = 7; // Hard: 10/hr, we stop at 7
  private readonly ERROR_WINDOW_MS = 60 * 60 * 1000; // 1 hour

  constructor(
    private readonly authService: ExactAuthService,
    private readonly httpService: HttpService,
    private readonly config: ConfigService,
  ) {
    this.baseUrl = this.config.get<string>('EXACT_BASE_URL')
      ?? 'https://start.exactonline.nl';
  }

  /**
   * Core request method. All Exact API calls go through here.
   * Handles: rate limiting -> auth header -> execute -> retry -> update counters.
   */
  async request<T>(
    method: string,
    path: string,
    body?: unknown,
    options?: { skipRateLimit?: boolean },
  ): Promise<{ data: T; headers: Record<string, string> }> {
    // 1. Check circuit breaker
    this.checkCircuitBreaker(path);

    // 2. Wait for rate limit clearance
    if (!options?.skipRateLimit) {
      await this.waitForRateLimit();
    }

    // 3. Get auth credentials
    const { apiKey, divisionId, connectionId } = await this.authService.getApiKey();
    const url = `${this.baseUrl}/api/v1/${divisionId}/${path}`;

    // 4. Execute with retry
    const startTime = Date.now();
    const response = await this.executeWithRetry(
      () => this.httpService.axiosRef.request({
        method,
        url,
        data: body,
        headers: {
          'Authorization': `Bearer ${apiKey}`,
          'Content-Type': 'application/json',
          'Accept': 'application/json',
        },
        timeout: 30_000,
      }),
      path,
      3, // maxRetries
    );
    const durationMs = Date.now() - startTime;

    // 5. Update rate limit counters from response headers
    this.updateRateLimitFromHeaders(response.headers as Record<string, string>);

    // 6. Persist daily counter
    await this.authService.incrementDailyCounter(connectionId);

    // 7. Reset endpoint error counter on success
    this.circuitBreaker.errorCounts.delete(this.normalizeEndpoint(path));

    return {
      data: response.data as T,
      headers: response.headers as Record<string, string>,
    };
  }

  /**
   * GET with OData pagination support.
   * Returns all pages concatenated.
   */
  async getAll<T>(path: string, params?: {
    select?: string;
    filter?: string;
    top?: number;
  }): Promise<T[]> {
    const results: T[] = [];
    let url = path;

    const queryParts: string[] = [];
    if (params?.select) queryParts.push(`$select=${params.select}`);
    if (params?.filter) queryParts.push(`$filter=${params.filter}`);
    if (params?.top) queryParts.push(`$top=${params.top}`);
    if (queryParts.length > 0) url += `?${queryParts.join('&')}`;

    do {
      const { data } = await this.request<{ d: { results: T[]; __next?: string } }>('GET', url);
      results.push(...data.d.results);
      url = data.d.__next ?? '';
    } while (url);

    return results;
  }

  /**
   * POST with GUID extraction from response.d.
   * Exact POST responses return the created entity under .d
   */
  async post<T>(path: string, body: unknown): Promise<T> {
    const { data } = await this.request<{ d: T }>('POST', path, body);
    return data.d;
  }

  /**
   * PUT for updates.
   * Returns 204 No Content on success (no response body).
   */
  async put(path: string, body: unknown): Promise<void> {
    await this.request<void>('PUT', path, body);
  }

  // ---------------------------------------------------------------------------
  // Rate Limiting
  // ---------------------------------------------------------------------------

  private async waitForRateLimit(): Promise<void> {
    const now = Date.now();

    // Reset minutely counter if window expired
    if (now > this.rateLimit.minutelyResetAt) {
      this.rateLimit.minutelyCount = 0;
      this.rateLimit.minutelyResetAt = now + 60_000;
    }

    // Check minutely limit (soft: 55/60)
    if (this.rateLimit.minutelyCount >= this.SOFT_MINUTELY_LIMIT) {
      const waitMs = this.rateLimit.minutelyResetAt - now;
      this.logger.warn(`Minutely rate limit approaching (${this.rateLimit.minutelyCount}/60), waiting ${waitMs}ms`);
      await this.sleep(Math.max(waitMs, 1000));
      this.rateLimit.minutelyCount = 0;
      this.rateLimit.minutelyResetAt = Date.now() + 60_000;
    }

    // Check daily limit (soft: 4950/5000)
    if (this.rateLimit.dailyRemaining <= (5000 - this.SOFT_DAILY_LIMIT)) {
      const waitMs = this.rateLimit.dailyResetAt - now;
      if (waitMs > 0) {
        this.logger.error(`Daily rate limit exhausted. Waiting until reset at ${new Date(this.rateLimit.dailyResetAt).toISOString()}`);
        throw new Error('Daily API rate limit exhausted. Try again tomorrow.');
      }
    }

    this.rateLimit.minutelyCount++;
  }

  /**
   * Parse BOTH minutely and daily rate limit headers.
   *
   * CRITICAL: Exact switches between header sets depending on proximity to limits.
   * Under normal conditions: X-RateLimit-Remaining (daily), X-RateLimit-Reset
   * Near minutely limit: X-RateLimit-Minutely-Remaining, X-RateLimit-Minutely-Reset
   *
   * Must check BOTH sets. Minutely takes priority when present.
   * (See API research: "Header-Gedreven Backoff Algoritmen")
   */
  private updateRateLimitFromHeaders(headers: Record<string, string>): void {
    // Minutely headers (only present when approaching minutely limit)
    const minutelyRemaining = headers['x-ratelimit-minutely-remaining'];
    if (minutelyRemaining !== undefined) {
      const remaining = parseInt(minutelyRemaining, 10);
      this.rateLimit.minutelyCount = 60 - remaining;

      const minutelyReset = headers['x-ratelimit-minutely-reset'];
      if (minutelyReset) {
        this.rateLimit.minutelyResetAt = parseInt(minutelyReset, 10) * 1000;
      }
    }

    // Daily headers (normally present)
    const dailyRemaining = headers['x-ratelimit-remaining'];
    if (dailyRemaining !== undefined) {
      this.rateLimit.dailyRemaining = parseInt(dailyRemaining, 10);
    }

    const dailyReset = headers['x-ratelimit-reset'];
    if (dailyReset) {
      this.rateLimit.dailyResetAt = parseInt(dailyReset, 10) * 1000;
    }
  }

  // ---------------------------------------------------------------------------
  // Retry with Exponential Backoff
  // ---------------------------------------------------------------------------

  private async executeWithRetry(
    fn: () => Promise<AxiosResponse>,
    path: string,
    maxRetries: number,
  ): Promise<AxiosResponse> {
    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      try {
        const response = await fn();

        // Detect 200 OK with empty results = permission issue
        if (response.status === 200 && response.data?.d?.results?.length === 0) {
          this.logger.warn(`Empty results from ${path} - possible permission issue`);
        }

        return response;
      } catch (error) {
        const axiosError = error as AxiosError;
        const status = axiosError.response?.status;

        // Track error for circuit breaker
        this.trackEndpointError(path);

        // Non-retriable errors: log, increment counter, throw immediately
        if (status === 400 || status === 401 || status === 403 || status === 404) {
          const errorBody = axiosError.response?.data as Record<string, unknown>;
          const errorMessage = this.parseExactError(errorBody);
          this.logger.error({ status, path, errorMessage }, 'Non-retriable Exact API error');
          throw new Error(`Exact API ${status}: ${errorMessage}`);
        }

        // Last attempt: throw
        if (attempt === maxRetries) {
          this.logger.error({ status, path, attempt }, 'Max retries exhausted');
          throw error;
        }

        // Retriable errors: 429, 408, 503, network errors
        if (status === 429) {
          const retryAfter = parseInt(
            (axiosError.response?.headers?.['retry-after'] as string) ?? '60',
            10,
          );
          this.logger.warn(`Rate limited (429), waiting ${retryAfter}s before retry ${attempt}/${maxRetries}`);
          await this.sleep(retryAfter * 1000);
          continue;
        }

        if (status === 408 || status === 503 || !status) {
          // Exponential backoff: 2s, 8s, 32s
          const backoffMs = Math.pow(4, attempt) * 500;
          this.logger.warn(`Retriable error (${status ?? 'network'}), backoff ${backoffMs}ms, attempt ${attempt}/${maxRetries}`);
          await this.sleep(backoffMs);
          continue;
        }

        // Unknown error: throw immediately
        throw error;
      }
    }

    throw new Error('Unreachable');
  }

  // ---------------------------------------------------------------------------
  // Circuit Breaker
  // ---------------------------------------------------------------------------

  /**
   * Track errors per endpoint. If 7 errors within 1 hour on the same endpoint,
   * the circuit opens and all calls to that endpoint are rejected.
   *
   * This protects against the Exact "Error Velocity" limit:
   * 10 errors/endpoint/hour = API key BLOCKED for 1 hour.
   * We stop at 7 to leave margin.
   */
  private trackEndpointError(path: string): void {
    const endpoint = this.normalizeEndpoint(path);
    const now = Date.now();
    const state = this.circuitBreaker.errorCounts.get(endpoint);

    if (!state || (now - state.firstErrorAt) > this.ERROR_WINDOW_MS) {
      // New window or expired window
      this.circuitBreaker.errorCounts.set(endpoint, { count: 1, firstErrorAt: now });
    } else {
      state.count++;
      if (state.count >= this.MAX_ERRORS_PER_ENDPOINT) {
        this.logger.error(
          `Circuit breaker OPEN for endpoint "${endpoint}" (${state.count} errors in ${Math.round((now - state.firstErrorAt) / 1000)}s)`,
        );
        this.circuitBreaker.isOpen = true;
        this.circuitBreaker.openUntil = state.firstErrorAt + this.ERROR_WINDOW_MS;
      }
    }
  }

  private checkCircuitBreaker(path: string): void {
    const endpoint = this.normalizeEndpoint(path);
    const state = this.circuitBreaker.errorCounts.get(endpoint);

    if (state && state.count >= this.MAX_ERRORS_PER_ENDPOINT) {
      const now = Date.now();
      if ((now - state.firstErrorAt) < this.ERROR_WINDOW_MS) {
        const remainingMin = Math.ceil((state.firstErrorAt + this.ERROR_WINDOW_MS - now) / 60_000);
        throw new Error(
          `Circuit breaker open for "${endpoint}". ${state.count} errors in the last hour. ` +
          `Blocked for ~${remainingMin} more minutes to prevent Exact API key suspension.`,
        );
      }
      // Window expired, reset
      this.circuitBreaker.errorCounts.delete(endpoint);
    }
  }

  /** Normalize path to endpoint key (e.g., "crm/Accounts?$filter=..." -> "crm/Accounts") */
  private normalizeEndpoint(path: string): string {
    return path.split('?')[0]!;
  }

  // ---------------------------------------------------------------------------
  // Error Parsing
  // ---------------------------------------------------------------------------

  /**
   * Parse Exact Online error responses.
   * The `error.code` field is ALWAYS empty. Must parse `error.message.value`.
   * (See API research: "Decodering van JSON Fout-Schema's")
   */
  private parseExactError(body: unknown): string {
    try {
      const err = body as { error?: { code?: string; message?: { value?: string } } };
      return err?.error?.message?.value ?? JSON.stringify(body);
    } catch {
      return 'Unknown Exact API error';
    }
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}
```

### 5c. ExactValidationService -- Pre-flight Checks

```typescript
import { Injectable, Logger } from '@nestjs/common';
import { DatabaseService } from '../database/database.service';

export interface ValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

/**
 * Pre-flight validation BEFORE any Exact API call.
 *
 * CRITICAL: Exact blocks the API key for 1 hour after 10 errors per endpoint.
 * Every bad request counts. Client-side validation is mandatory.
 * (See API research: "De Foutfrequentie-valstrik en Defensief Programmeren")
 */
@Injectable()
export class ExactValidationService {
  private readonly logger = new Logger(ExactValidationService.name);

  constructor(private readonly db: DatabaseService) {}

  /**
   * Validate a relation before POST to crm/Accounts.
   */
  validateAccount(relation: {
    name: string;
    exactSupplierId?: string | null;
    exactCustomerId?: string | null;
    isSupplier: boolean;
    isCustomer: boolean;
    country?: string | null;
    vatNumber?: string | null;
  }): ValidationResult {
    const errors: string[] = [];
    const warnings: string[] = [];

    // Exact mandatory: Name (max 50 chars)
    if (!relation.name?.trim()) {
      errors.push('Name is required for Exact Account');
    } else if (relation.name.length > 50) {
      errors.push(`Name exceeds 50 characters: "${relation.name.substring(0, 50)}..."`);
    }

    // Code is required (max 18 chars) - use exactSupplierId or exactCustomerId
    const code = relation.exactSupplierId || relation.exactCustomerId;
    if (!code) {
      errors.push('No Exact code (exactSupplierId or exactCustomerId) available');
    } else if (code.length > 18) {
      errors.push(`Exact code exceeds 18 characters: "${code}"`);
    }

    // At least one role required
    if (!relation.isSupplier && !relation.isCustomer) {
      errors.push('Relation must be a supplier or customer (or both)');
    }

    // Country code format warning
    if (relation.country && relation.country.length > 2) {
      warnings.push(`Country "${relation.country}" may need ISO 3166-1 alpha-2 conversion`);
    }

    return { valid: errors.length === 0, errors, warnings };
  }

  /**
   * Validate an article before POST to logistics/Items.
   */
  validateItem(article: {
    exactItemCode: string | null;
    description: string;
    unit: string;
  }): ValidationResult {
    const errors: string[] = [];
    const warnings: string[] = [];

    // Code is required (max 30 chars)
    if (!article.exactItemCode?.trim()) {
      errors.push('exactItemCode is required for Exact Item');
    } else if (article.exactItemCode.length > 30) {
      errors.push(`Item code exceeds 30 characters: "${article.exactItemCode}"`);
    }

    // Description is required (max 60 chars for Exact)
    if (!article.description?.trim()) {
      errors.push('Description is required for Exact Item');
    } else if (article.description.length > 60) {
      warnings.push(`Description will be truncated to 60 chars for Exact`);
    }

    // Unit mapping check
    if (!article.unit) {
      errors.push('Unit is required');
    }
    // Note: Unit mapping (SUPWISE -> Exact codes) needs verification in Step 6

    return { valid: errors.length === 0, errors, warnings };
  }

  /**
   * Validate a full order before push to Exact (SO + POs).
   * Checks all dependencies: customer GUID, all article GUIDs, all supplier GUIDs.
   */
  async validateSalesOrder(orderId: string): Promise<ValidationResult> {
    const errors: string[] = [];
    const warnings: string[] = [];

    const order = await this.db.order.findUnique({
      where: { id: orderId },
      include: {
        customer: {
          select: {
            id: true, name: true, exactAccountGuid: true,
            exactSupplierId: true, exactCustomerId: true,
          },
        },
        lines: {
          orderBy: { sortOrder: 'asc' },
          include: {
            article: {
              select: {
                id: true, wsyArticleCode: true, description: true,
                exactItemGuid: true, exactItemCode: true,
              },
            },
            supplier: {
              select: {
                id: true, name: true, exactAccountGuid: true,
                exactSupplierId: true,
              },
            },
          },
        },
        purchaseOrders: { select: { id: true, supplierId: true } },
      },
    });

    if (!order) {
      return { valid: false, errors: ['Order not found'], warnings: [] };
    }

    // 1. Customer must have Exact GUID
    if (!order.customer.exactAccountGuid) {
      errors.push(`Customer "${order.customer.name}" has no Exact Account GUID. Sync relations first.`);
    }

    // 2. All order lines must have article + article GUID
    const missingArticleGuids = new Set<string>();
    const missingSupplierGuids = new Set<string>();

    for (const line of order.lines) {
      if (!line.article) {
        errors.push(`Line ${line.lineNumber} has no article linked`);
        continue;
      }
      if (!line.article.exactItemGuid) {
        missingArticleGuids.add(line.article.wsyArticleCode);
      }
      if (line.supplier && !line.supplier.exactAccountGuid) {
        missingSupplierGuids.add(line.supplier.name);
      }
    }

    if (missingArticleGuids.size > 0) {
      errors.push(
        `${missingArticleGuids.size} article(s) missing Exact Item GUID: ` +
        `${[...missingArticleGuids].slice(0, 5).join(', ')}` +
        (missingArticleGuids.size > 5 ? ` (+${missingArticleGuids.size - 5} more)` : ''),
      );
    }

    if (missingSupplierGuids.size > 0) {
      errors.push(
        `${missingSupplierGuids.size} supplier(s) missing Exact Account GUID: ` +
        `${[...missingSupplierGuids].slice(0, 5).join(', ')}` +
        (missingSupplierGuids.size > 5 ? ` (+${missingSupplierGuids.size - 5} more)` : ''),
      );
    }

    // 3. Order should have purchase orders before push
    if (order.purchaseOrders.length === 0) {
      warnings.push('No purchase orders generated yet. Only Sales Order will be pushed.');
    }

    // 4. Check for duplicate push (already synced)
    if (order.exactSalesOrderId) {
      warnings.push(
        `Order already pushed to Exact (SO ID: ${order.exactSalesOrderId}). ` +
        `Pushing again will create a duplicate in Exact.`,
      );
    }

    // 5. Validate line quantities/prices
    for (const line of order.lines) {
      if (!line.unitSellPrice || Number(line.unitSellPrice) <= 0) {
        warnings.push(`Line ${line.lineNumber}: no sell price set`);
      }
    }

    return { valid: errors.length === 0, errors, warnings };
  }
}
```

### 5d. ExactSyncService -- Bulk Sync

```typescript
import { Injectable, Logger } from '@nestjs/common';
import { DatabaseService } from '../database/database.service';
import { ExactApiService } from './exact-api.service';
import { ExactValidationService } from './exact-validation.service';
import { ExactSyncLogService } from './exact-sync-log.service';

export interface SyncProgress {
  jobId: string;
  entityType: 'account' | 'item';
  total: number;
  synced: number;
  failed: number;
  skipped: number;        // Already have GUID
  errors: Array<{ entityId: string; name: string; error: string }>;
  startedAt: Date;
  completedAt?: Date;
}

@Injectable()
export class ExactSyncService {
  private readonly logger = new Logger(ExactSyncService.name);

  // In-memory progress tracking (for polling from frontend)
  private activeJobs = new Map<string, SyncProgress>();

  constructor(
    private readonly db: DatabaseService,
    private readonly exactApi: ExactApiService,
    private readonly validation: ExactValidationService,
    private readonly syncLog: ExactSyncLogService,
  ) {}

  // -------------------------------------------------------------------------
  // Relations -> Exact Accounts
  // -------------------------------------------------------------------------

  /**
   * Sync all active relations to Exact as Accounts.
   * - Idempotent: skips relations that already have exactAccountGuid
   * - Pre-validates each relation before sending (prevent 400s -> API key block)
   * - Groups by exactSupplierId/exactCustomerId for deduplication
   *
   * Pattern follows ArticlesImportService.importFromCsv() batched approach
   * (see articles-import.service.ts lines 134-156)
   */
  async syncRelations(userId: string): Promise<SyncProgress> {
    const jobId = crypto.randomUUID();

    const relations = await this.db.relation.findMany({
      where: {
        exactAccountGuid: null,    // Not yet synced
        isActive: true,
        OR: [
          { exactSupplierId: { not: null } },
          { exactCustomerId: { not: null } },
        ],
      },
      orderBy: { name: 'asc' },
    });

    const progress: SyncProgress = {
      jobId,
      entityType: 'account',
      total: relations.length,
      synced: 0,
      failed: 0,
      skipped: 0,
      errors: [],
      startedAt: new Date(),
    };
    this.activeJobs.set(jobId, progress);

    for (const relation of relations) {
      try {
        // Pre-flight validation (prevents wasted API calls)
        const validation = this.validation.validateAccount(relation);
        if (!validation.valid) {
          progress.failed++;
          progress.errors.push({
            entityId: relation.id,
            name: relation.name,
            error: `Validation: ${validation.errors.join('; ')}`,
          });
          await this.syncLog.log({
            entityType: 'account',
            entityId: relation.id,
            action: 'create',
            status: 'failed',
            errorMessage: `Pre-flight validation failed: ${validation.errors.join('; ')}`,
            createdBy: userId,
          });
          continue;
        }

        const payload = this.buildAccountPayload(relation);
        const startMs = Date.now();
        const result = await this.exactApi.post<{ ID: string }>('crm/Accounts', payload);
        const durationMs = Date.now() - startMs;

        // Store GUID on relation
        await this.db.relation.update({
          where: { id: relation.id },
          data: {
            exactAccountGuid: result.ID,
            exactSyncedAt: new Date(),
          },
        });

        await this.syncLog.log({
          entityType: 'account',
          entityId: relation.id,
          action: 'create',
          status: 'success',
          requestBody: payload,
          responseBody: result as unknown as Record<string, unknown>,
          durationMs,
          createdBy: userId,
        });

        progress.synced++;
      } catch (error) {
        progress.failed++;
        const errorMsg = error instanceof Error ? error.message : String(error);
        progress.errors.push({
          entityId: relation.id,
          name: relation.name,
          error: errorMsg,
        });

        await this.syncLog.log({
          entityType: 'account',
          entityId: relation.id,
          action: 'create',
          status: 'failed',
          errorMessage: errorMsg,
          requestBody: this.buildAccountPayload(relation),
          createdBy: userId,
        });

        // If circuit breaker opened, stop the entire sync
        if (errorMsg.includes('Circuit breaker open')) {
          this.logger.error('Circuit breaker opened during relation sync. Stopping.');
          break;
        }
      }
    }

    progress.completedAt = new Date();
    this.logger.log(
      `Relation sync complete: ${progress.synced} synced, ${progress.failed} failed out of ${progress.total}`,
    );

    return progress;
  }

  // -------------------------------------------------------------------------
  // Articles -> Exact Items (with deduplication)
  // -------------------------------------------------------------------------

  /**
   * Sync all active articles to Exact as Items.
   * - DEDUPLICATION: Multiple SUPWISE articles may share the same exact_item_code.
   *   We POST once per unique exact_item_code, then assign the returned GUID to ALL
   *   articles sharing that code.
   * - Uses exactItemCode as the Exact Item.Code field
   * - Idempotent: skips articles already having exactItemGuid
   */
  async syncArticles(userId: string): Promise<SyncProgress> {
    const jobId = crypto.randomUUID();

    const articles = await this.db.article.findMany({
      where: {
        exactItemGuid: null,
        exactItemCode: { not: null },
        isActive: true,
      },
      select: {
        id: true,
        exactItemCode: true,
        description: true,
        unit: true,
        eanCode: true,
        weightPerItemKg: true,
        countryOfOrigin: true,
        hsCode: true,
      },
      orderBy: { exactItemCode: 'asc' },
    });

    // Group by exact_item_code for deduplication
    const codeGroups = new Map<string, typeof articles>();
    for (const article of articles) {
      const code = article.exactItemCode!;
      if (!codeGroups.has(code)) codeGroups.set(code, []);
      codeGroups.get(code)!.push(article);
    }

    const progress: SyncProgress = {
      jobId,
      entityType: 'item',
      total: codeGroups.size,    // Unique codes, not unique articles
      synced: 0,
      failed: 0,
      skipped: 0,
      errors: [],
      startedAt: new Date(),
    };
    this.activeJobs.set(jobId, progress);

    for (const [code, groupArticles] of codeGroups) {
      try {
        const firstArticle = groupArticles[0]!;

        // Pre-flight validation
        const validation = this.validation.validateItem(firstArticle);
        if (!validation.valid) {
          progress.failed++;
          progress.errors.push({
            entityId: firstArticle.id,
            name: code,
            error: `Validation: ${validation.errors.join('; ')}`,
          });
          continue;
        }

        const payload = this.buildItemPayload(firstArticle);
        const startMs = Date.now();
        const result = await this.exactApi.post<{ ID: string }>('logistics/Items', payload);
        const durationMs = Date.now() - startMs;

        const guid = result.ID;

        // Assign GUID to ALL articles with this code (batch update)
        await this.db.article.updateMany({
          where: { exactItemCode: code, exactItemGuid: null },
          data: {
            exactItemGuid: guid,
            exactSyncedAt: new Date(),
          },
        });

        await this.syncLog.log({
          entityType: 'item',
          entityId: firstArticle.id,
          action: 'create',
          status: 'success',
          requestBody: payload,
          responseBody: result as unknown as Record<string, unknown>,
          durationMs,
          createdBy: userId,
        });

        progress.synced++;
      } catch (error) {
        progress.failed++;
        const errorMsg = error instanceof Error ? error.message : String(error);
        progress.errors.push({
          entityId: groupArticles[0]!.id,
          name: code,
          error: errorMsg,
        });

        await this.syncLog.log({
          entityType: 'item',
          entityId: groupArticles[0]!.id,
          action: 'create',
          status: 'failed',
          errorMessage: errorMsg,
          createdBy: userId,
        });

        if (errorMsg.includes('Circuit breaker open')) {
          this.logger.error('Circuit breaker opened during article sync. Stopping.');
          break;
        }
      }
    }

    progress.completedAt = new Date();
    this.logger.log(
      `Article sync complete: ${progress.synced} synced, ${progress.failed} failed out of ${progress.total} unique codes`,
    );

    return progress;
  }

  // -------------------------------------------------------------------------
  // Progress Polling
  // -------------------------------------------------------------------------

  getSyncProgress(jobId: string): SyncProgress | null {
    return this.activeJobs.get(jobId) ?? null;
  }

  // -------------------------------------------------------------------------
  // Payload Builders
  // -------------------------------------------------------------------------

  private buildAccountPayload(relation: {
    name: string;
    exactSupplierId: string | null;
    exactCustomerId: string | null;
    isSupplier: boolean;
    isCustomer: boolean;
    country: string | null;
    city: string | null;
    postalCode: string | null;
    phone: string | null;
    email: string | null;
    vatNumber: string | null;
    street: string | null;
  }) {
    return {
      Code: relation.exactSupplierId || relation.exactCustomerId,
      Name: relation.name.substring(0, 50),  // Exact max 50 chars
      IsSupplier: relation.isSupplier,
      IsPurchase: relation.isSupplier,        // Exact's "is leverancier"
      IsSales: relation.isCustomer,           // Exact's "is klant"
      // Optional fields -- undefined values are omitted by JSON.stringify
      Country: relation.country?.substring(0, 2) ?? undefined,  // ISO 3166-1 alpha-2
      City: relation.city ?? undefined,
      Postcode: relation.postalCode ?? undefined,
      Phone: relation.phone ?? undefined,
      Email: relation.email ?? undefined,
      VATNumber: relation.vatNumber ?? undefined,
    };
  }

  private buildItemPayload(article: {
    exactItemCode: string | null;
    description: string;
    unit: string;
  }) {
    return {
      Code: article.exactItemCode,
      Description: article.description.substring(0, 60),  // Exact max 60 chars
      IsSalesItem: true,
      IsPurchaseItem: true,
      // Unit mapping: SUPWISE unit -> Exact unit code
      // TODO: Verify Exact unit codes in Step 6 (first API call test)
      // Preliminary mapping based on common Exact unit codes:
      // pc -> pc, kg -> kg, ltr -> l, box -> box, set -> set, m -> m, m2 -> m2, m3 -> m3
    };
  }
}
```

### 5e. ExactPushService -- Order Push (SO + POs)

```typescript
import { BadRequestException, Injectable, Logger, NotFoundException } from '@nestjs/common';
import { DatabaseService } from '../database/database.service';
import { ExactApiService } from './exact-api.service';
import { ExactValidationService } from './exact-validation.service';
import { ExactSyncLogService } from './exact-sync-log.service';

interface PushResult {
  salesOrder: {
    exactId: string;
    exactNumber: number;
    status: 'synced';
  };
  purchaseOrders: Array<{
    poId: string;
    poNumber: number;
    exactId: string | null;
    status: 'synced' | 'sync_failed';
    error?: string;
  }>;
}

/**
 * Push a SUPWISE order to Exact Online.
 * Fan-out pattern:
 *   1 SUPWISE Order -> 1 Exact Sales Order (all lines with sell prices)
 *                   -> N Exact Purchase Orders (1 per supplier, purchase prices)
 *
 * Follows PurchaseOrdersService.generatePurchaseOrders() pattern
 * (purchase-orders.service.ts lines 285-419):
 * - Group lines by supplier
 * - Create in sequence (not parallel, to respect rate limits)
 * - Partial failure: SO succeeds, some POs fail -> mark individually
 */
@Injectable()
export class ExactPushService {
  private readonly logger = new Logger(ExactPushService.name);

  constructor(
    private readonly db: DatabaseService,
    private readonly exactApi: ExactApiService,
    private readonly validation: ExactValidationService,
    private readonly syncLog: ExactSyncLogService,
  ) {}

  /**
   * Preview what will be pushed (no writes to Exact).
   * Returns dependency check + what SO and POs would look like.
   * Similar to PurchaseOrdersService.previewPurchaseOrders() (line 183-279).
   */
  async previewPush(orderId: string) {
    const validationResult = await this.validation.validateSalesOrder(orderId);

    const order = await this.db.order.findUnique({
      where: { id: orderId },
      include: {
        customer: { select: { id: true, name: true, exactAccountGuid: true } },
        lines: {
          orderBy: { sortOrder: 'asc' },
          include: {
            article: { select: { id: true, wsyArticleCode: true, exactItemGuid: true } },
            supplier: { select: { id: true, name: true, exactAccountGuid: true } },
          },
        },
        purchaseOrders: {
          include: {
            supplier: { select: { id: true, name: true, exactAccountGuid: true } },
            lines: { include: { article: { select: { id: true, wsyArticleCode: true } } } },
          },
        },
      },
    });

    return {
      validation: validationResult,
      salesOrder: order ? {
        customer: order.customer.name,
        lineCount: order.lines.length,
        totalSell: Number(order.totalSellAmount),
      } : null,
      purchaseOrders: (order?.purchaseOrders ?? []).map((po) => ({
        poId: po.id,
        supplier: po.supplier.name,
        lineCount: po.lines.length,
        hasExactGuid: !!po.supplier.exactAccountGuid,
      })),
      alreadySynced: order?.exactSalesOrderId != null,
    };
  }

  /**
   * Push order to Exact: Sales Order first, then Purchase Orders sequentially.
   */
  async pushOrder(orderId: string, userId: string): Promise<PushResult> {
    // 1. Full validation
    const validationResult = await this.validation.validateSalesOrder(orderId);
    if (!validationResult.valid) {
      throw new BadRequestException({
        message: 'Cannot push to Exact: validation failed',
        errors: validationResult.errors,
      });
    }

    // 2. Load full order with all relations
    const order = await this.db.order.findUnique({
      where: { id: orderId },
      include: {
        customer: true,
        lines: {
          orderBy: { sortOrder: 'asc' },
          include: {
            article: true,
            supplier: true,
          },
        },
        purchaseOrders: {
          include: {
            supplier: true,
            lines: {
              orderBy: { sortOrder: 'asc' },
              include: { article: true },
            },
          },
        },
      },
    });

    if (!order) throw new NotFoundException('Order not found');

    // 3. Build and POST Sales Order
    const soPayload = {
      OrderedBy: order.customer.exactAccountGuid,
      Description: `SUPWISE ${order.orderNumber}`,
      YourRef: order.reference ?? `Order ${order.orderNumber}`,
      SalesOrderLines: order.lines
        .filter((line) => line.article?.exactItemGuid)
        .map((line) => ({
          Item: line.article!.exactItemGuid,
          Description: (line.descriptionSnapshot ?? '').substring(0, 60),
          Quantity: Number(line.quantity),
          NetPrice: Number(line.unitSellPrice ?? 0),
        })),
    };

    const startMs = Date.now();
    const soResult = await this.exactApi.post<{
      OrderID: string;
      OrderNumber: number;
      SalesOrderLines: { results: Array<{ ID: string }> };
    }>('salesorder/SalesOrders', soPayload);
    const soDurationMs = Date.now() - startMs;

    // 4. Update order with Exact IDs
    await this.db.order.update({
      where: { id: orderId },
      data: {
        exactSalesOrderId: soResult.OrderID,
        exactSalesOrderNumber: soResult.OrderNumber,
        exactSyncStatus: 'synced',
        exactLastSyncedAt: new Date(),
      },
    });

    // 4b. Update individual line GUIDs if returned
    if (soResult.SalesOrderLines?.results) {
      const soLines = soResult.SalesOrderLines.results;
      const orderLines = order.lines.filter((l) => l.article?.exactItemGuid);
      for (let i = 0; i < Math.min(soLines.length, orderLines.length); i++) {
        await this.db.orderLine.update({
          where: { id: orderLines[i]!.id },
          data: { exactSalesLineId: soLines[i]!.ID },
        });
      }
    }

    await this.syncLog.log({
      entityType: 'sales_order',
      entityId: orderId,
      action: 'create',
      status: 'success',
      requestBody: soPayload,
      responseBody: soResult as unknown as Record<string, unknown>,
      durationMs: soDurationMs,
      createdBy: userId,
    });

    await this.db.logAudit(userId, 'EXACT_PUSH', 'order', orderId, {
      exactSalesOrderId: soResult.OrderID,
      exactSalesOrderNumber: soResult.OrderNumber,
    });

    // 5. Push Purchase Orders (sequential, not parallel -- rate limit protection)
    const poResults: PushResult['purchaseOrders'] = [];

    for (const po of order.purchaseOrders) {
      try {
        if (!po.supplier.exactAccountGuid) {
          throw new Error(`Supplier "${po.supplier.name}" has no Exact Account GUID`);
        }

        const poPayload = {
          Supplier: po.supplier.exactAccountGuid,
          Description: `SUPWISE PO ${po.poNumber}`,
          PurchaseOrderLines: po.lines
            .filter((line) => line.article?.exactItemGuid)
            .map((line) => ({
              Item: (line.article as { exactItemGuid: string }).exactItemGuid,
              Description: line.descriptionSnapshot.substring(0, 60),
              QuantityInPurchaseUnits: Number(line.quantity),
              NetPrice: Number(line.unitPurchasePrice),
            })),
        };

        const poStartMs = Date.now();
        const poResult = await this.exactApi.post<{
          PurchaseOrderID: string;
          OrderNumber?: number;
          PurchaseOrderLines?: { results: Array<{ ID: string }> };
        }>('purchaseorder/PurchaseOrders', poPayload);
        const poDurationMs = Date.now() - poStartMs;

        await this.db.purchaseOrder.update({
          where: { id: po.id },
          data: {
            exactPurchaseOrderId: poResult.PurchaseOrderID,
            exactPurchaseOrderNumber: poResult.OrderNumber ?? null,
            exactSyncStatus: 'synced',
            exactLastSyncedAt: new Date(),
          },
        });

        // Update PO line GUIDs if returned
        if (poResult.PurchaseOrderLines?.results) {
          const poLines = poResult.PurchaseOrderLines.results;
          for (let i = 0; i < Math.min(poLines.length, po.lines.length); i++) {
            await this.db.purchaseOrderLine.update({
              where: { id: po.lines[i]!.id },
              data: { exactPurchaseLineId: poLines[i]!.ID },
            });
          }
        }

        await this.syncLog.log({
          entityType: 'purchase_order',
          entityId: po.id,
          action: 'create',
          status: 'success',
          requestBody: poPayload,
          responseBody: poResult as unknown as Record<string, unknown>,
          durationMs: poDurationMs,
          createdBy: userId,
        });

        poResults.push({
          poId: po.id,
          poNumber: po.poNumber,
          exactId: poResult.PurchaseOrderID,
          status: 'synced',
        });
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error);

        await this.db.purchaseOrder.update({
          where: { id: po.id },
          data: { exactSyncStatus: 'sync_failed' },
        });

        await this.syncLog.log({
          entityType: 'purchase_order',
          entityId: po.id,
          action: 'create',
          status: 'failed',
          errorMessage: errorMsg,
          createdBy: userId,
        });

        poResults.push({
          poId: po.id,
          poNumber: po.poNumber,
          exactId: null,
          status: 'sync_failed',
          error: errorMsg,
        });

        // If circuit breaker opened, stop remaining POs
        if (errorMsg.includes('Circuit breaker open')) {
          this.logger.error('Circuit breaker opened during PO push. Remaining POs skipped.');
          break;
        }
      }
    }

    return {
      salesOrder: {
        exactId: soResult.OrderID,
        exactNumber: soResult.OrderNumber,
        status: 'synced',
      },
      purchaseOrders: poResults,
    };
  }

  /**
   * Retry failed POs for an order.
   * Useful when some POs failed during initial push.
   */
  async retryFailedPOs(orderId: string, userId: string, poIds?: string[]) {
    const order = await this.db.order.findUnique({
      where: { id: orderId },
      include: {
        purchaseOrders: {
          where: {
            exactSyncStatus: 'sync_failed',
            ...(poIds?.length ? { id: { in: poIds } } : {}),
          },
          include: {
            supplier: true,
            lines: {
              orderBy: { sortOrder: 'asc' },
              include: { article: true },
            },
          },
        },
      },
    });

    if (!order) throw new NotFoundException('Order not found');

    // Re-use the same push logic for each failed PO
    const results: Array<{ poId: string; status: string; error?: string }> = [];

    for (const po of order.purchaseOrders) {
      try {
        if (!po.supplier.exactAccountGuid) {
          throw new Error(`Supplier has no Exact GUID`);
        }

        const poPayload = {
          Supplier: po.supplier.exactAccountGuid,
          Description: `SUPWISE PO ${po.poNumber}`,
          PurchaseOrderLines: po.lines
            .filter((line) => (line.article as { exactItemGuid: string | null })?.exactItemGuid)
            .map((line) => ({
              Item: (line.article as { exactItemGuid: string }).exactItemGuid,
              Description: line.descriptionSnapshot.substring(0, 60),
              QuantityInPurchaseUnits: Number(line.quantity),
              NetPrice: Number(line.unitPurchasePrice),
            })),
        };

        const result = await this.exactApi.post<{ PurchaseOrderID: string }>(
          'purchaseorder/PurchaseOrders',
          poPayload,
        );

        await this.db.purchaseOrder.update({
          where: { id: po.id },
          data: {
            exactPurchaseOrderId: result.PurchaseOrderID,
            exactSyncStatus: 'synced',
            exactLastSyncedAt: new Date(),
          },
        });

        results.push({ poId: po.id, status: 'synced' });
      } catch (error) {
        const errorMsg = error instanceof Error ? error.message : String(error);
        results.push({ poId: po.id, status: 'sync_failed', error: errorMsg });
      }
    }

    return { retried: results.length, results };
  }
}
```

### 5f. ExactSyncLogService

```typescript
import { Injectable } from '@nestjs/common';
import { DatabaseService } from '../database/database.service';

@Injectable()
export class ExactSyncLogService {
  constructor(private readonly db: DatabaseService) {}

  async log(entry: {
    entityType: string;
    entityId: string;
    action: string;
    status: string;
    attempt?: number;
    errorMessage?: string;
    errorCode?: string;
    requestBody?: unknown;
    responseBody?: unknown;
    durationMs?: number;
    createdBy: string;
  }) {
    return this.db.exactSyncLog.create({
      data: {
        entityType: entry.entityType,
        entityId: entry.entityId,
        action: entry.action,
        status: entry.status,
        attempt: entry.attempt ?? 1,
        errorMessage: entry.errorMessage,
        errorCode: entry.errorCode,
        requestBody: entry.requestBody as any,
        responseBody: entry.responseBody as any,
        durationMs: entry.durationMs,
        createdBy: entry.createdBy,
      },
    });
  }

  async findAll(filters?: {
    entityType?: string;
    status?: string;
    entityId?: string;
    limit?: number;
    cursor?: string;
  }) {
    const where: Record<string, unknown> = {};
    if (filters?.entityType) where.entityType = filters.entityType;
    if (filters?.status) where.status = filters.status;
    if (filters?.entityId) where.entityId = filters.entityId;

    const limit = filters?.limit ?? 100;

    return this.db.exactSyncLog.findMany({
      where,
      orderBy: { createdAt: 'desc' },
      take: limit + 1,
      ...(filters?.cursor ? { cursor: { id: filters.cursor }, skip: 1 } : {}),
      include: {
        createdByUser: { select: { id: true, fullName: true } },
      },
    });
  }

  async getStats() {
    const [totalSuccess, totalFailed, recentErrors] = await Promise.all([
      this.db.exactSyncLog.count({ where: { status: 'success' } }),
      this.db.exactSyncLog.count({ where: { status: 'failed' } }),
      this.db.exactSyncLog.findMany({
        where: { status: 'failed' },
        orderBy: { createdAt: 'desc' },
        take: 10,
        select: {
          id: true,
          entityType: true,
          entityId: true,
          errorMessage: true,
          createdAt: true,
        },
      }),
    ]);

    return { totalSuccess, totalFailed, recentErrors };
  }
}
```

---

## 6. Controllers

### 6a. ExactController (Admin: connect, disconnect, status)

```typescript
import { Body, Controller, Delete, Get, Post } from '@nestjs/common';
import { Roles } from '../common/decorators/roles.decorator';
import { CurrentUser } from '../common/decorators/current-user.decorator';
import { AuthenticatedUser } from '../common/interfaces/authenticated-user.interface';
import { ExactAuthService } from './exact-auth.service';
import { ConnectExactDto } from './dto/connect-exact.dto';

@Controller('exact')
export class ExactController {
  constructor(private readonly authService: ExactAuthService) {}

  @Post('connect')
  @Roles('admin')
  connect(@Body() dto: ConnectExactDto, @CurrentUser() user: AuthenticatedUser) {
    return this.authService.connect(dto.apiKey, dto.divisionId, user.id);
  }

  @Get('status')
  @Roles('admin')
  getStatus() {
    return this.authService.getStatus();
  }

  @Delete('disconnect')
  @Roles('admin')
  disconnect(@CurrentUser() user: AuthenticatedUser) {
    return this.authService.disconnect(user.id);
  }
}
```

### 6b. ExactSyncController

```typescript
import { Controller, Get, Param, Post } from '@nestjs/common';
import { Roles } from '../common/decorators/roles.decorator';
import { CurrentUser } from '../common/decorators/current-user.decorator';
import { AuthenticatedUser } from '../common/interfaces/authenticated-user.interface';
import { ExactSyncService } from './exact-sync.service';
import { ExactSyncLogService } from './exact-sync-log.service';

@Controller('exact/sync')
export class ExactSyncController {
  constructor(
    private readonly syncService: ExactSyncService,
    private readonly syncLogService: ExactSyncLogService,
  ) {}

  @Post('relations')
  @Roles('admin')
  syncRelations(@CurrentUser() user: AuthenticatedUser) {
    return this.syncService.syncRelations(user.id);
  }

  @Post('articles')
  @Roles('admin')
  syncArticles(@CurrentUser() user: AuthenticatedUser) {
    return this.syncService.syncArticles(user.id);
  }

  @Get('progress/:jobId')
  @Roles('admin')
  getProgress(@Param('jobId') jobId: string) {
    return this.syncService.getSyncProgress(jobId);
  }

  @Get('log')
  @Roles('admin')
  getSyncLog() {
    return this.syncLogService.findAll();
  }

  @Get('stats')
  @Roles('admin')
  getSyncStats() {
    return this.syncLogService.getStats();
  }
}
```

### 6c. ExactPushController

```typescript
import { Controller, Get, Param, ParseUUIDPipe, Post, Body } from '@nestjs/common';
import { Roles } from '../common/decorators/roles.decorator';
import { CurrentUser } from '../common/decorators/current-user.decorator';
import { AuthenticatedUser } from '../common/interfaces/authenticated-user.interface';
import { ExactPushService } from './exact-push.service';
import { QUOTE_ORDER_ROLES } from '../common/constants/rbac';

const PUSH_ROLES = ['admin', 'manager_senior'] as const;

@Controller('exact/push')
export class ExactPushController {
  constructor(private readonly pushService: ExactPushService) {}

  @Get('preview/:orderId')
  @Roles(...PUSH_ROLES)
  previewPush(@Param('orderId', ParseUUIDPipe) orderId: string) {
    return this.pushService.previewPush(orderId);
  }

  @Post('order/:orderId')
  @Roles(...PUSH_ROLES)
  pushOrder(
    @Param('orderId', ParseUUIDPipe) orderId: string,
    @CurrentUser() user: AuthenticatedUser,
  ) {
    return this.pushService.pushOrder(orderId, user.id);
  }

  @Post('retry/:orderId')
  @Roles(...PUSH_ROLES)
  retryFailedPOs(
    @Param('orderId', ParseUUIDPipe) orderId: string,
    @Body() body: { purchaseOrderIds?: string[] },
    @CurrentUser() user: AuthenticatedUser,
  ) {
    return this.pushService.retryFailedPOs(orderId, user.id, body.purchaseOrderIds);
  }
}
```

---

## 7. DTOs

### `connect-exact.dto.ts`

```typescript
import { IsInt, IsString, MaxLength, Min, MinLength } from 'class-validator';

export class ConnectExactDto {
  @IsString()
  @MinLength(10)
  @MaxLength(500)
  apiKey!: string;

  @IsInt()
  @Min(1)
  divisionId!: number;
}
```

### `list-sync-log.dto.ts`

```typescript
import { IsIn, IsInt, IsOptional, IsString, IsUUID, Max, Min } from 'class-validator';
import { Type } from 'class-transformer';

export class ListSyncLogDto {
  @IsOptional()
  @IsString()
  @IsIn(['account', 'item', 'sales_order', 'purchase_order'])
  entityType?: string;

  @IsOptional()
  @IsString()
  @IsIn(['success', 'failed', 'retrying'])
  status?: string;

  @IsOptional()
  @IsUUID()
  entityId?: string;

  @IsOptional()
  @Type(() => Number)
  @IsInt()
  @Min(1)
  @Max(500)
  limit?: number;

  @IsOptional()
  @IsUUID()
  cursor?: string;
}
```

---

## 8. Rate Limiting Strategy (Summary)

| Dimension | Soft Limit | Hard Limit | Enforcement |
|-----------|-----------|------------|-------------|
| **Minutely** | 55 req/min | 60 req/min | In-memory counter, reset from `X-RateLimit-Minutely-Reset` header |
| **Daily** | 4,950 req/day | 5,000 req/day | DB-persisted counter in `exact_connections.daily_calls_used`, reset at midnight (from `X-RateLimit-Reset` header) |
| **Error velocity** | 7 per endpoint/hour | 10 per endpoint/hour = API key BLOCKED 1 hour | Per-endpoint in-memory tracker with 1-hour sliding window |
| **Circuit breaker** | Opens at 7 consecutive errors OR 7 total per endpoint per hour | N/A | Rejects all calls to that endpoint until the 1-hour window expires |

**Dual header monitoring implementation:**
- Check `X-RateLimit-Minutely-Remaining` FIRST (takes priority when present)
- Fall back to `X-RateLimit-Remaining` for daily tracking
- When minutely header appears, it means Exact has switched mode -- the daily headers are temporarily absent

---

## 9. Error Handling Strategy

| HTTP Status | Classification | Action | Retry? |
|-------------|---------------|--------|--------|
| 200 + empty results | Permission issue | Log warning, return empty | No |
| 400 Bad Request | Validation error | Log full error, increment error counter, throw | No |
| 401 Unauthorized | Auth error | Log, throw, surface "check API key" to admin | No |
| 403 Forbidden | Division/scope error | Log, throw, surface to admin | No |
| 404 Not Found | Entity not found | Log, skip, continue | No |
| 408 Request Timeout | Server slow | Backoff 2s, 8s, 32s | Yes (3x) |
| 429 Too Many Requests | Rate limited | Wait `Retry-After` seconds | Yes (3x) |
| 500 Internal Server Error | DB constraint/server bug | Log, check if "Data already exists" | No (usually) |
| 503 Service Unavailable | Server overloaded | Backoff 2s, 8s, 32s | Yes (3x) |

**Error message parsing:**
```typescript
// Exact always returns: { error: { code: "", message: { value: "actual error" } } }
// The code field is ALWAYS empty. Parse message.value for diagnostics.
```

---

## 10. Integration Points

### Changes to existing modules

**NO CHANGES needed to `OrdersService` or `PurchaseOrdersService`.** The Exact integration is fully decoupled:
- ExactModule reads from the same database but operates independently
- Push is triggered by the user from dedicated endpoints, not from order/PO lifecycle hooks
- The `outdated` detection trigger is a DB-level concern (no service changes needed)

**Pino logger redaction** (`app.module.ts` line 39-50): Add API key to redaction paths:

```typescript
redact: {
  paths: [
    // ... existing paths ...
    'req.body.apiKey',         // Exact connect endpoint
  ],
  censor: '***',
},
```

### Frontend API Endpoints (for Next.js frontend team)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/api/exact/connect` | Admin only | Store API key + division ID |
| `GET` | `/api/exact/status` | Admin only | Connection status + sync stats |
| `DELETE` | `/api/exact/disconnect` | Admin only | Remove connection |
| `POST` | `/api/exact/sync/relations` | Admin only | Start relation sync (long-running) |
| `POST` | `/api/exact/sync/articles` | Admin only | Start article sync (long-running) |
| `GET` | `/api/exact/sync/progress/:jobId` | Admin only | Poll sync progress |
| `GET` | `/api/exact/sync/log` | Admin only | Sync log with filters |
| `GET` | `/api/exact/sync/stats` | Admin only | Aggregate sync statistics |
| `GET` | `/api/exact/push/preview/:orderId` | Admin, Manager Senior | Preview push + dependency check |
| `POST` | `/api/exact/push/order/:orderId` | Admin, Manager Senior | Push order to Exact |
| `POST` | `/api/exact/push/retry/:orderId` | Admin, Manager Senior | Retry failed POs |

---

## 11. Security Considerations

1. **API key encryption:** AES-256-GCM with random IV per connection. Auth tag prevents tampering.
2. **EXACT_ENCRYPTION_KEY:** Must be generated with `openssl rand -hex 32` and stored in environment, never in code.
3. **Never log API key:** Pino redaction for `req.body.apiKey`. Services never include key in audit logs or sync logs.
4. **RLS on sensitive tables:** Both `exact_connections` and `exact_sync_log` have RLS enabled with service-role-only policies.
5. **Admin-only access:** All Exact endpoints require `@Roles('admin')` or `@Roles('admin', 'manager_senior')`.
6. **Request body logging:** Sync log stores request/response bodies but NEVER stores the API key (it's only in the Authorization header, which Pino already redacts).

---

## 12. Implementation Sequence

| Step | What | Depends On | Estimate |
|------|------|------------|----------|
| 1 | Write SQL migration + review | -- | 0.5 day |
| 2 | Apply migration to test division | Step 1 | 0.5 day |
| 3 | Update Prisma schema + `npx prisma generate` | Step 2 | 0.5 day |
| 4 | `ExactAuthService` -- API key encrypt/decrypt | Step 3 | 0.5 day |
| 5 | `ExactApiService` -- HTTP client + dual rate limiting + circuit breaker | Step 4 | 1 day |
| 6 | **VERIFICATION: First API call to Exact test division** | Step 5 + API key | 0.5 day |
| 7 | `ExactValidationService` -- pre-flight checks | Step 3 | 0.5 day |
| 8 | `ExactSyncLogService` -- log CRUD | Step 3 | 0.5 day |
| 9 | `ExactSyncService` -- relations sync | Steps 5, 7, 8 | 1 day |
| 10 | `ExactSyncService` -- articles sync + deduplication | Step 9 | 1.5 days |
| 11 | **VERIFICATION: Bulk sync test with real data** | Step 10 | 1 day |
| 12 | `ExactPushService` -- order push (SO + POs) | Steps 9, 10 | 1.5 days |
| 13 | Controllers + DTOs + Module wiring | Steps 4-12 | 0.5 day |
| 14 | `app.module.ts` registration + config schema update | Step 13 | 0.5 day |
| 15 | Frontend: Admin connection + sync dashboard | Step 14 | 1.5 days |
| 16 | Frontend: Order push button + status badges | Step 14 | 1 day |
| 17 | Frontend: GUID indicators on relations/articles | Step 14 | 0.5 day |
| **Total** | | | **~12 working days** |

**Critical path:** Step 6 is the go/no-go gate. If the auth header format or basic API call does not work, everything stops there for investigation.

---

## 13. Open Questions (to verify during Step 6)

| # | Question | Blocking For |
|---|----------|-------------|
| Q1 | Exact auth header format -- is it `Bearer {key}`, `Basic {key}`, or custom? | Step 5-6 |
| Q2 | Required fields for POST to `crm/Accounts` | Step 9 |
| Q3 | Required fields for POST to `logistics/Items` | Step 10 |
| Q4 | Required fields for POST to `salesorder/SalesOrders` | Step 12 |
| Q5 | Unit code mapping (SUPWISE `pc,kg,ltr` to Exact codes) | Step 10 |
| Q6 | Country code format (ISO2, ISO3, or full name?) | Step 9 |
| Q7 | Division ID retrieval via `current/Me` endpoint | Step 4 |
| Q8 | Account Code padding rules (18 chars with leading spaces?) | Step 9 |
