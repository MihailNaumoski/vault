# Phase 5B — Exact Online Koppeling: Implementatieplan

> **Versie:** 1.0  
> **Datum:** 2026-04-07  
> **Gebaseerd op:** Meeting uitkomsten met Exact (gecorrigeerde feiten), huidige SUPWISE codebase  
> **Status:** Gereed voor implementatie

---

## 1. Overzicht

Phase 5B bouwt de Exact Online koppeling voor SUPWISE. SUPWISE is het systeem van record voor orders, relaties en artikelen. Exact Online is de downstream boekhoudkundige representatie.

**Wat wordt er gebouwd:**

1. **Initiële bulk-sync** — Alle relaties en artikelen worden eenmalig naar Exact gestuurd via POST. Exact geeft per entiteit een GUID terug. Die GUIDs worden opgeslagen in SUPWISE als permanente referentie.

2. **Order push** — Na de initiële sync kan een gebruiker een bevestigde order handmatig naar Exact sturen. Dit maakt één Sales Order en N Purchase Orders aan in Exact (één per leverancier op de order).

3. **Sync tracking** — Elke push-poging wordt gelogd. Orders krijgen een sync-status (`not_synced`, `synced`, `sync_failed`, `outdated`).

**Scope v1:**
- Handmatige push per order ("Push naar Exact" knop)
- Bulk-sync van relaties en artikelen (admin-only, eenmalig)
- Sync log en status indicatoren in de UI

**Buiten scope v1:**
- Automatische/geplande sync
- Pull van wijzigingen vanuit Exact naar SUPWISE
- Webhooks
- Bijwerken van orders na eerste push (re-push)
- Facturatiemodule
- Drop Shipments

---

## 2. Architectuurbeslissingen

### 2.1 API Key in plaats van OAuth2

**Originele spec:** OAuth2 authorization code flow met `EXACT_CLIENT_ID`, `EXACT_CLIENT_SECRET`, `EXACT_REDIRECT_URI`.

**Gecorrigeerd (na meeting):** Exact Online biedt voor privé app-registraties een directe API key via het Exact App Center. Geen OAuth2 consent flow, geen access/refresh tokens, geen token-refresh logica. De API key is permanent totdat hij handmatig wordt ingetrokken.

**Impact:**
- `exact-auth.service.ts` hoeft géén token-refresh te implementeren
- `EXACT_REDIRECT_URI` env var wordt verwijderd
- `exact_connections` tabel slaat de versleutelde API key op (niet OAuth2 tokens)
- Env var `EXACT_ENCRYPTION_KEY` toegevoegd voor AES-256-GCM encryptie van de API key

### 2.2 Geen speciaal sync-endpoint

**Gecorrigeerd (na meeting):** Er is geen Exact "sync" of "bulk import" endpoint. We gebruiken standaard REST: POST per entiteit, sla GUID op.

**Endpoints:**
| Doel | HTTP | URL |
|------|------|-----|
| Relatie aanmaken | POST | `/api/v1/{division}/crm/Accounts` |
| Artikel aanmaken | POST | `/api/v1/{division}/logistics/Items` |
| Verkooporder aanmaken | POST | `/api/v1/{division}/salesorder/SalesOrders` |
| Inkooporder aanmaken | POST | `/api/v1/{division}/purchaseorder/PurchaseOrders` |

### 2.3 GUID-strategie

**Gecorrigeerd (na meeting):** Items bestaan NIET in de testdivisie. Alles moet via POST aangemaakt worden. De GUID wordt teruggegeven in de response.

**Huidige database-situatie:**
- `relations.exact_supplier_id` en `relations.exact_customer_id` — TEXT velden die CSV-codes bevatten (bijv. `"20180"`), GEEN GUIDs
- `articles.exact_item_code` — TEXT veld met de Exact artikelcode (bijv. `"8701003844"`), NIET uniek

**Beslissing:** Twee nieuwe GUID-kolommen toevoegen naast de bestaande velden:
- `relations.exact_account_guid` — GUID teruggegeven door Exact na POST (zowel voor klant als leverancier, Exact kent één Account per relatie)
- `articles.exact_item_guid` — GUID teruggegeven door Exact na POST

De bestaande velden `exact_supplier_id`, `exact_customer_id` en `exact_item_code` worden **niet verwijderd** — ze blijven als leesbare referentiecode voor menselijke traceerbaarheid.

### 2.4 Artikel-deduplicatie bij sync

`exact_item_code` is **niet uniek** in SUPWISE: meerdere SUPWISE artikelen kunnen hetzelfde Exact artikelcode delen. Bij de bulk-sync geldt:
- Groepeer per unieke `exact_item_code`
- POST één Exact Item per unieke code
- Sla de teruggegeven GUID op in `exact_item_guid` voor **alle** artikelen met die code

Dit voorkomt duplicaten in Exact. Artikelen zonder `exact_item_code` worden voorlopig overgeslagen en gerapporteerd.

### 2.5 Rate limits en bulk-sync aanpak

Per meeting: hoog volume is acceptabel bij de initiële eenmalige bulk-import. Daarna gelden normale limieten (60/min, 5000/dag).

Voor ~200k artikelen bij 60/min = ~3.333 minuten = ~56 uur. Aanpak:
- Bulk-sync draait als achtergrondproces, niet als synchrone HTTP request
- Progress wordt opgeslagen in de `exact_connections` tabel (cursor: laatste gesynchroniseerde ID)
- Resume na failure: overgeslagen entities (niet-null `exact_account_guid`/`exact_item_guid`)
- Concurrency configureerbaar via env var `EXACT_SYNC_BATCH_SIZE` (default: 10 parallel)

---

## 3. Database Migraties

Alle migraties zijn Supabase-migrations (SQL). Prisma schema dient APART bijgewerkt te worden door de backend developer.

### 3.1 Nieuwe tabel: `exact_connections`

```sql
-- Migration: 20260407_exact_connections

CREATE TABLE exact_connections (
  id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  api_key_enc     TEXT        NOT NULL,              -- AES-256-GCM encrypted API key
  division_id     INTEGER     NOT NULL,              -- Exact Online divisie ID
  base_url        TEXT        NOT NULL DEFAULT 'https://start.exactonline.nl',
  is_active       BOOLEAN     NOT NULL DEFAULT true,
  connected_by    UUID        NOT NULL REFERENCES auth.users(id),
  -- Sync progress cursors (voor resume)
  relation_sync_cursor   UUID NULL,                  -- Laatste gesynchroniseerde relation.id
  article_sync_cursor    TEXT NULL,                  -- Laatste gesynchroniseerde exact_item_code
  relation_sync_status   TEXT NOT NULL DEFAULT 'pending'
    CHECK (relation_sync_status IN ('pending','running','done','failed')),
  article_sync_status    TEXT NOT NULL DEFAULT 'pending'
    CHECK (article_sync_status IN ('pending','running','done','failed')),
  relation_sync_total    INTEGER NULL,               -- Totaal te verwerken
  relation_sync_done     INTEGER NOT NULL DEFAULT 0, -- Verwerkt
  article_sync_total     INTEGER NULL,
  article_sync_done      INTEGER NOT NULL DEFAULT 0,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Maximaal één actieve connectie per organisatie
CREATE UNIQUE INDEX idx_exact_connections_active
  ON exact_connections (is_active)
  WHERE is_active = true;

CREATE INDEX idx_exact_connections_division ON exact_connections (division_id);

-- RLS: alleen admins
ALTER TABLE exact_connections ENABLE ROW LEVEL SECURITY;

CREATE POLICY "exact_connections_admin_only"
  ON exact_connections
  FOR ALL
  TO authenticated
  USING (
    EXISTS (
      SELECT 1 FROM user_profiles
      WHERE id = auth.uid()
        AND role = 'admin'
    )
  );

-- Updated_at trigger
CREATE TRIGGER trg_exact_connections_updated_at
  BEFORE UPDATE ON exact_connections
  FOR EACH ROW
  EXECUTE FUNCTION update_updated_at_column();
```

### 3.2 Nieuwe kolommen op `relations`

```sql
-- Migration: 20260407_relations_exact_guid

ALTER TABLE relations
  ADD COLUMN exact_account_guid    TEXT NULL,   -- Exact Online Account GUID (UUID als string)
  ADD COLUMN exact_synced_at       TIMESTAMPTZ NULL;  -- Tijdstip laatste succesvolle sync

-- Index voor snelle lookup "niet gesynchroniseerd" bij bulk-sync
CREATE INDEX idx_relations_exact_sync
  ON relations (exact_account_guid)
  WHERE exact_account_guid IS NULL AND is_active = true;

-- Index voor GUID-lookup bij order push
CREATE INDEX idx_relations_exact_account_guid
  ON relations (exact_account_guid)
  WHERE exact_account_guid IS NOT NULL;
```

### 3.3 Nieuwe kolommen op `articles`

```sql
-- Migration: 20260407_articles_exact_guid

ALTER TABLE articles
  ADD COLUMN exact_item_guid       TEXT NULL,   -- Exact Online Item GUID (UUID als string)
  ADD COLUMN exact_item_synced_at  TIMESTAMPTZ NULL;

-- Index voor bulk-sync: artikelen zonder GUID, gegroepeerd per item code
CREATE INDEX idx_articles_exact_sync
  ON articles (exact_item_code, exact_item_guid)
  WHERE exact_item_guid IS NULL
    AND exact_item_code IS NOT NULL
    AND is_active = true;

-- Index voor GUID-lookup bij order push
CREATE INDEX idx_articles_exact_item_guid
  ON articles (exact_item_guid)
  WHERE exact_item_guid IS NOT NULL;
```

### 3.4 Nieuwe kolommen op `orders`

```sql
-- Migration: 20260407_orders_exact_sync

ALTER TABLE orders
  ADD COLUMN exact_sales_order_id      TEXT NULL,     -- Exact Online SalesOrder GUID
  ADD COLUMN exact_sales_order_number  INTEGER NULL,  -- Exact Online leesbaar ordernummer
  ADD COLUMN exact_sync_status         TEXT NOT NULL DEFAULT 'not_synced'
    CHECK (exact_sync_status IN ('not_synced','synced','sync_failed','outdated')),
  ADD COLUMN exact_last_synced_at      TIMESTAMPTZ NULL;

CREATE INDEX idx_orders_exact_sync_status
  ON orders (exact_sync_status);

CREATE INDEX idx_orders_exact_sales_order_id
  ON orders (exact_sales_order_id)
  WHERE exact_sales_order_id IS NOT NULL;
```

### 3.5 Nieuwe kolommen op `order_lines`

```sql
-- Migration: 20260407_order_lines_exact_sync

ALTER TABLE order_lines
  ADD COLUMN exact_sales_line_id   TEXT NULL;   -- Exact Online SalesOrderLine GUID

CREATE INDEX idx_order_lines_exact_sales_line_id
  ON order_lines (exact_sales_line_id)
  WHERE exact_sales_line_id IS NOT NULL;
```

### 3.6 Nieuwe kolommen op `purchase_orders`

```sql
-- Migration: 20260407_purchase_orders_exact_sync

ALTER TABLE purchase_orders
  ADD COLUMN exact_purchase_order_id   TEXT NULL,   -- Exact Online PurchaseOrder GUID
  ADD COLUMN exact_sync_status         TEXT NOT NULL DEFAULT 'not_synced'
    CHECK (exact_sync_status IN ('not_synced','synced','sync_failed','outdated')),
  ADD COLUMN exact_last_synced_at      TIMESTAMPTZ NULL;

CREATE INDEX idx_purchase_orders_exact_sync_status
  ON purchase_orders (exact_sync_status);

CREATE INDEX idx_purchase_orders_exact_po_id
  ON purchase_orders (exact_purchase_order_id)
  WHERE exact_purchase_order_id IS NOT NULL;
```

### 3.7 Nieuwe tabel: `exact_sync_log`

```sql
-- Migration: 20260407_exact_sync_log

CREATE TABLE exact_sync_log (
  id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  entity_type     TEXT        NOT NULL
    CHECK (entity_type IN ('relation','article','sales_order','purchase_order')),
  entity_id       UUID        NOT NULL,          -- SUPWISE interne ID
  action          TEXT        NOT NULL
    CHECK (action IN ('create','update','delete')),
  status          TEXT        NOT NULL
    CHECK (status IN ('success','failed','retrying')),
  attempt         INTEGER     NOT NULL DEFAULT 1,
  error_code      TEXT NULL,                     -- HTTP status code als string ('400','429',etc.)
  error_message   TEXT NULL,                     -- Foutomschrijving
  request_body    JSONB NULL,                    -- Payload gestuurd naar Exact (geen secrets)
  response_body   JSONB NULL,                    -- Response van Exact
  exact_guid      TEXT NULL,                     -- Ontvangen GUID bij succes
  triggered_by    UUID NULL REFERENCES auth.users(id),
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Primaire query: logs per entity
CREATE INDEX idx_exact_sync_log_entity
  ON exact_sync_log (entity_type, entity_id, created_at DESC);

-- Admin dashboard: recente failures
CREATE INDEX idx_exact_sync_log_status_date
  ON exact_sync_log (status, created_at DESC);

-- RLS: alle geauthenticeerde gebruikers kunnen lezen (sync log is niet gevoelig)
ALTER TABLE exact_sync_log ENABLE ROW LEVEL SECURITY;

CREATE POLICY "exact_sync_log_read_all"
  ON exact_sync_log
  FOR SELECT
  TO authenticated
  USING (true);

CREATE POLICY "exact_sync_log_insert_backend"
  ON exact_sync_log
  FOR INSERT
  TO service_role
  WITH CHECK (true);
```

### 3.8 Overzicht Prisma schema wijzigingen

De backend developer dient de volgende modellen bij te werken in `packages/database/prisma/schema.prisma`:

**`Relation` model — toevoegen:**
```prisma
exactAccountGuid   String?   @map("exact_account_guid")
exactSyncedAt      DateTime? @map("exact_synced_at") @db.Timestamptz
```

**`Article` model — toevoegen:**
```prisma
exactItemGuid      String?   @map("exact_item_guid")
exactItemSyncedAt  DateTime? @map("exact_item_synced_at") @db.Timestamptz
```

**`Order` model — toevoegen:**
```prisma
exactSalesOrderId     String?   @map("exact_sales_order_id")
exactSalesOrderNumber Int?      @map("exact_sales_order_number")
exactSyncStatus       String    @default("not_synced") @map("exact_sync_status")
exactLastSyncedAt     DateTime? @map("exact_last_synced_at") @db.Timestamptz
```

**`OrderLine` model — toevoegen:**
```prisma
exactSalesLineId   String?   @map("exact_sales_line_id")
```

**`PurchaseOrder` model — toevoegen:**
```prisma
exactPurchaseOrderId  String?   @map("exact_purchase_order_id")
exactSyncStatus       String    @default("not_synced") @map("exact_sync_status")
exactLastSyncedAt     DateTime? @map("exact_last_synced_at") @db.Timestamptz
```

**Nieuw model `ExactConnection`:**
```prisma
model ExactConnection {
  id                   String    @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  apiKeyEnc            String    @map("api_key_enc")
  divisionId           Int       @map("division_id")
  baseUrl              String    @default("https://start.exactonline.nl") @map("base_url")
  isActive             Boolean   @default(true) @map("is_active")
  connectedBy          String    @map("connected_by") @db.Uuid
  relationSyncCursor   String?   @map("relation_sync_cursor") @db.Uuid
  articleSyncCursor    String?   @map("article_sync_cursor")
  relationSyncStatus   String    @default("pending") @map("relation_sync_status")
  articleSyncStatus    String    @default("pending") @map("article_sync_status")
  relationSyncTotal    Int?      @map("relation_sync_total")
  relationSyncDone     Int       @default(0) @map("relation_sync_done")
  articleSyncTotal     Int?      @map("article_sync_total")
  articleSyncDone      Int       @default(0) @map("article_sync_done")
  createdAt            DateTime  @default(now()) @map("created_at") @db.Timestamptz
  updatedAt            DateTime  @updatedAt @map("updated_at") @db.Timestamptz

  @@index([isActive])
  @@map("exact_connections")
}
```

**Nieuw model `ExactSyncLog`:**
```prisma
model ExactSyncLog {
  id           String    @id @default(dbgenerated("gen_random_uuid()")) @db.Uuid
  entityType   String    @map("entity_type")
  entityId     String    @map("entity_id") @db.Uuid
  action       String
  status       String
  attempt      Int       @default(1)
  errorCode    String?   @map("error_code")
  errorMessage String?   @map("error_message")
  requestBody  Json?     @map("request_body")
  responseBody Json?     @map("response_body")
  exactGuid    String?   @map("exact_guid")
  triggeredBy  String?   @map("triggered_by") @db.Uuid
  createdAt    DateTime  @default(now()) @map("created_at") @db.Timestamptz

  @@index([entityType, entityId])
  @@index([status, createdAt(sort: Desc)])
  @@map("exact_sync_log")
}
```

---

## 4. Backend Implementatie

### 4a. Nieuwe NestJS Module: ExactModule

**Module structuur:**

```
apps/api/src/exact/
├── exact.module.ts
├── exact.controller.ts
├── services/
│   ├── exact-auth.service.ts        # API key opslag, encryptie, ophalen
│   ├── exact-api.service.ts         # HTTP client, rate limiting, retry
│   ├── exact-sync.service.ts        # Bulk-sync relaties + artikelen
│   ├── exact-push.service.ts        # Order push naar Exact
│   └── exact-sync-log.service.ts    # Sync log CRUD
├── dto/
│   ├── connect-exact.dto.ts         # POST /exact/connect body
│   ├── push-order-response.dto.ts   # Response shape voor push
│   └── sync-log-query.dto.ts        # GET /exact/sync-log query params
└── interfaces/
    ├── exact-account.interface.ts   # Payload voor POST /crm/Accounts
    ├── exact-item.interface.ts      # Payload voor POST /logistics/Items
    ├── exact-sales-order.interface.ts
    └── exact-purchase-order.interface.ts
```

**`exact.module.ts`:**
```typescript
import { Module } from '@nestjs/common';
import { HttpModule } from '@nestjs/axios';
import { ExactController } from './exact.controller';
import { ExactAuthService } from './services/exact-auth.service';
import { ExactApiService } from './services/exact-api.service';
import { ExactSyncService } from './services/exact-sync.service';
import { ExactPushService } from './services/exact-push.service';
import { ExactSyncLogService } from './services/exact-sync-log.service';
import { DatabaseModule } from '../database/database.module';

@Module({
  imports: [
    HttpModule.register({ timeout: 30_000 }),
    DatabaseModule,
  ],
  controllers: [ExactController],
  providers: [
    ExactAuthService,
    ExactApiService,
    ExactSyncService,
    ExactPushService,
    ExactSyncLogService,
  ],
  exports: [ExactAuthService, ExactApiService],
})
export class ExactModule {}
```

**Registreer in `app.module.ts`** — toevoegen na `DashboardModule`:
```typescript
import { ExactModule } from './exact/exact.module';
// ... in imports array:
ExactModule,
```

---

### 4b. API Key Management

**Encryptie:** AES-256-GCM via Node.js `crypto`. De encryption key komt uit de env var `EXACT_ENCRYPTION_KEY` (32-byte hex string = 64 chars).

**`exact-auth.service.ts`:**
```typescript
import { Injectable, NotFoundException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { DatabaseService } from '../../database/database.service';
import { createCipheriv, createDecipheriv, randomBytes } from 'crypto';

@Injectable()
export class ExactAuthService {
  private readonly algorithm = 'aes-256-gcm';
  private readonly encKey: Buffer;

  constructor(
    private readonly config: ConfigService,
    private readonly db: DatabaseService,
  ) {
    const keyHex = this.config.getOrThrow<string>('EXACT_ENCRYPTION_KEY');
    if (keyHex.length !== 64) throw new Error('EXACT_ENCRYPTION_KEY must be 64 hex chars (32 bytes)');
    this.encKey = Buffer.from(keyHex, 'hex');
  }

  encrypt(plaintext: string): string {
    const iv = randomBytes(12); // 96-bit nonce for GCM
    const cipher = createCipheriv(this.algorithm, this.encKey, iv);
    const encrypted = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final()]);
    const tag = cipher.getAuthTag();
    // Format: iv:tag:ciphertext (all hex)
    return `${iv.toString('hex')}:${tag.toString('hex')}:${encrypted.toString('hex')}`;
  }

  decrypt(encoded: string): string {
    const [ivHex, tagHex, cipherHex] = encoded.split(':');
    const iv = Buffer.from(ivHex, 'hex');
    const tag = Buffer.from(tagHex, 'hex');
    const ciphertext = Buffer.from(cipherHex, 'hex');
    const decipher = createDecipheriv(this.algorithm, this.encKey, iv);
    decipher.setAuthTag(tag);
    return decipher.update(ciphertext) + decipher.final('utf8');
  }

  async saveConnection(apiKey: string, divisionId: number, userId: string): Promise<void> {
    const apiKeyEnc = this.encrypt(apiKey);
    // Deactiveer bestaande connecties
    await this.db.$executeRaw`UPDATE exact_connections SET is_active = false`;
    await this.db.exactConnection.create({
      data: { apiKeyEnc, divisionId, connectedBy: userId, isActive: true },
    });
  }

  async getApiKey(): Promise<string> {
    const conn = await this.db.exactConnection.findFirst({
      where: { isActive: true },
      select: { apiKeyEnc: true },
    });
    if (!conn) throw new NotFoundException('Geen actieve Exact Online koppeling gevonden');
    return this.decrypt(conn.apiKeyEnc);
  }

  async getConnection() {
    return this.db.exactConnection.findFirst({
      where: { isActive: true },
    });
  }

  async isConnected(): Promise<boolean> {
    const count = await this.db.exactConnection.count({ where: { isActive: true } });
    return count > 0;
  }
}
```

**Config schema updates** (`apps/api/src/common/config/config.schema.ts`):
```typescript
// Toevoegen aan het Joi object:
EXACT_ENCRYPTION_KEY: Joi.string().length(64).optional(),  // Vereist zodra Exact koppeling gebruikt wordt
EXACT_BASE_URL: Joi.string().uri().default('https://start.exactonline.nl'),
EXACT_SYNC_BATCH_SIZE: Joi.number().integer().min(1).max(50).default(10),
```

**`.env.example` wijzigingen:**
```bash
# Exact Online API (Phase 5B)
# EXACT_CLIENT_ID en EXACT_CLIENT_SECRET zijn verwijderd (geen OAuth2 meer)
# EXACT_REDIRECT_URI is verwijderd (geen OAuth2 meer)
EXACT_API_KEY=           # API key uit Exact App Center (opslaan via /api/exact/connect)
EXACT_ENCRYPTION_KEY=    # 32-byte hex (64 chars) — genereer met: openssl rand -hex 32
EXACT_BASE_URL=https://start.exactonline.nl
EXACT_SYNC_BATCH_SIZE=10
```

---

### 4c. Exact API Client Service

**`exact-api.service.ts`:**

```typescript
import { Injectable, Logger } from '@nestjs/common';
import { HttpService } from '@nestjs/axios';
import { ConfigService } from '@nestjs/config';
import { firstValueFrom } from 'rxjs';
import { ExactAuthService } from './exact-auth.service';

interface RateState {
  minuteCount: number;
  minuteReset: number;  // unix timestamp ms
  dayCount: number;
  dayReset: number;     // unix timestamp ms (midnight)
}

@Injectable()
export class ExactApiService {
  private readonly logger = new Logger(ExactApiService.name);
  private rateState: RateState = {
    minuteCount: 0,
    minuteReset: 0,
    dayCount: 0,
    dayReset: 0,
  };

  constructor(
    private readonly http: HttpService,
    private readonly auth: ExactAuthService,
    private readonly config: ConfigService,
  ) {}

  // ---------------------------------------------------------------------------
  // Core request method
  // ---------------------------------------------------------------------------

  async request<T>(
    method: 'GET' | 'POST' | 'PUT' | 'DELETE',
    path: string,       // bijv. '/crm/Accounts'
    body?: unknown,
    retryCount = 0,
  ): Promise<T> {
    await this.waitForRateLimit();

    const conn = await this.auth.getConnection();
    if (!conn) throw new Error('Geen Exact Online koppeling');

    const apiKey = await this.auth.getApiKey();
    const baseUrl = conn.baseUrl;
    const url = `${baseUrl}/api/v1/${conn.divisionId}${path}`;

    try {
      const response = await firstValueFrom(
        this.http.request<T>({
          method,
          url,
          data: body,
          headers: {
            Authorization: `Bearer ${apiKey}`,
            'Content-Type': 'application/json',
            Accept: 'application/json',
          },
        }),
      );

      this.updateRateState(response.headers);
      return response.data;
    } catch (error: any) {
      const status = error?.response?.status;
      const MAX_RETRIES = 3;

      if (status === 429 && retryCount < MAX_RETRIES) {
        const delay = Math.pow(4, retryCount) * 1000; // 1s, 4s, 16s
        this.logger.warn(`Rate limited (429). Retry ${retryCount + 1}/${MAX_RETRIES} in ${delay}ms`);
        await this.sleep(delay);
        return this.request<T>(method, path, body, retryCount + 1);
      }

      if (status >= 500 && retryCount < MAX_RETRIES) {
        const delay = Math.pow(2, retryCount) * 1000; // 1s, 2s, 4s
        this.logger.warn(`Server error (${status}). Retry ${retryCount + 1}/${MAX_RETRIES} in ${delay}ms`);
        await this.sleep(delay);
        return this.request<T>(method, path, body, retryCount + 1);
      }

      throw this.normalizeError(error);
    }
  }

  // Convenience methods
  async post<T>(path: string, body: unknown): Promise<T> {
    return this.request<T>('POST', path, body);
  }

  async get<T>(path: string): Promise<T> {
    return this.request<T>('GET', path);
  }

  // ---------------------------------------------------------------------------
  // Rate limiting (in-memory, single-instance)
  // ---------------------------------------------------------------------------

  private async waitForRateLimit(): Promise<void> {
    const now = Date.now();

    // Reset minutely counter
    if (now > this.rateState.minuteReset) {
      this.rateState.minuteCount = 0;
      this.rateState.minuteReset = now + 60_000;
    }

    // Reset daily counter at midnight
    const midnight = this.getNextMidnight();
    if (now > this.rateState.dayReset) {
      this.rateState.dayCount = 0;
      this.rateState.dayReset = midnight;
    }

    // Wacht als per-minuut limiet bereikt (58 voor marge)
    if (this.rateState.minuteCount >= 58) {
      const waitMs = this.rateState.minuteReset - now + 100;
      this.logger.warn(`Minutely rate limit bereikt. Wacht ${waitMs}ms`);
      await this.sleep(waitMs);
      this.rateState.minuteCount = 0;
      this.rateState.minuteReset = Date.now() + 60_000;
    }

    // Stop als dagelijks limiet bereikt (4900 voor marge)
    if (this.rateState.dayCount >= 4900) {
      throw new Error('Dagelijks Exact API limiet bereikt (4900 calls). Probeer morgen opnieuw.');
    }

    this.rateState.minuteCount++;
    this.rateState.dayCount++;
  }

  private updateRateState(headers: Record<string, string>): void {
    // Exact geeft rate limit headers terug — gebruik deze als ze beschikbaar zijn
    const minuteRemaining = parseInt(headers['x-ratelimit-minutely-remaining'] ?? '-1');
    const dayRemaining = parseInt(headers['x-ratelimit-daily-remaining'] ?? '-1');

    if (minuteRemaining >= 0) {
      this.rateState.minuteCount = 60 - minuteRemaining;
    }
    if (dayRemaining >= 0) {
      this.rateState.dayCount = 5000 - dayRemaining;
    }
  }

  private getNextMidnight(): number {
    const d = new Date();
    d.setHours(24, 0, 0, 0);
    return d.getTime();
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  // ---------------------------------------------------------------------------
  // Error normalization
  // ---------------------------------------------------------------------------

  private normalizeError(error: any): Error {
    const status = error?.response?.status;
    const message = error?.response?.data?.error?.message
      ?? error?.response?.data?.Message
      ?? error?.message
      ?? 'Onbekende Exact API fout';

    const err = new Error(`Exact API ${status ?? 'error'}: ${message}`);
    (err as any).exactStatus = status;
    (err as any).exactResponse = error?.response?.data;
    return err;
  }
}
```

**Error codes en afhandeling:**

| HTTP Status | Situatie | Actie |
|-------------|----------|-------|
| 200/201 | Succes | Verwerk response, sla GUID op |
| 400 | Validatiefout (verplichte velden, ongeldige waarden) | Log fout, markeer `sync_failed`, toon in UI |
| 401 | API key ongeldig of verlopen | Gooi `UnauthorizedException`, markeer connectie als inactief |
| 403 | Onvoldoende rechten | Log fout, flag voor admin |
| 404 | Entiteit niet gevonden (bij PUT) | Log fout, verwerk als creatie-poging |
| 429 | Rate limit bereikt | Retry met exponential backoff (1s → 4s → 16s) |
| 500-503 | Server error bij Exact | Retry tot 3x (1s → 2s → 4s), daarna `sync_failed` |

---

### 4d. Initiële Sync Service

**`exact-sync.service.ts`:**

```typescript
import { Injectable, Logger } from '@nestjs/common';
import { DatabaseService } from '../../database/database.service';
import { ExactApiService } from './exact-api.service';
import { ExactSyncLogService } from './exact-sync-log.service';

interface ExactAccountResponse {
  d: { ID: string; Code: string; Name: string };
}

interface ExactItemResponse {
  d: { ID: string; Code: string; Description: string };
}

@Injectable()
export class ExactSyncService {
  private readonly logger = new Logger(ExactSyncService.name);

  constructor(
    private readonly db: DatabaseService,
    private readonly api: ExactApiService,
    private readonly syncLog: ExactSyncLogService,
  ) {}

  // ---------------------------------------------------------------------------
  // SYNC RELATIES → Exact Accounts
  // ---------------------------------------------------------------------------

  /**
   * Bulk-sync alle SUPWISE relaties zonder exact_account_guid naar Exact.
   * Verwerkt in batches. Sla cursor op voor resume na failure.
   * Roep aan als achtergrondproces (niet await in HTTP request).
   */
  async syncAllRelations(triggeredBy: string): Promise<void> {
    const conn = await this.db.exactConnection.findFirst({ where: { isActive: true } });
    if (!conn) throw new Error('Geen actieve Exact koppeling');

    // Tel totaal te verwerken
    const total = await this.db.relation.count({
      where: { exactAccountGuid: null, isActive: true },
    });

    await this.db.exactConnection.update({
      where: { id: conn.id },
      data: { relationSyncStatus: 'running', relationSyncTotal: total, relationSyncDone: 0 },
    });

    let processed = 0;
    let cursor: string | undefined = conn.relationSyncCursor ?? undefined;

    try {
      while (true) {
        const batch = await this.db.relation.findMany({
          where: { exactAccountGuid: null, isActive: true },
          orderBy: { id: 'asc' },
          take: 50,
          ...(cursor ? { cursor: { id: cursor }, skip: 1 } : {}),
          select: {
            id: true,
            name: true,
            isSupplier: true,
            isCustomer: true,
            vatNumber: true,
            email: true,
            phone: true,
            street: true,
            houseNumber: true,
            postalCode: true,
            city: true,
            country: true,
            exactSupplierId: true,  // Legacy CSV code — meesturen als Code naar Exact
            exactCustomerId: true,
          },
        });

        if (batch.length === 0) break;

        for (const relation of batch) {
          try {
            const payload = this.buildAccountPayload(relation);
            const response = await this.api.post<ExactAccountResponse>('/crm/Accounts', payload);
            const guid = response.d.ID;

            await this.db.relation.update({
              where: { id: relation.id },
              data: {
                exactAccountGuid: guid,
                exactSyncedAt: new Date(),
              },
            });

            await this.syncLog.log({
              entityType: 'relation',
              entityId: relation.id,
              action: 'create',
              status: 'success',
              exactGuid: guid,
              requestBody: payload,
              responseBody: response.d,
              triggeredBy,
            });

            processed++;
            cursor = relation.id;

            // Sla cursor op elke 10 records (zodat resume goed werkt)
            if (processed % 10 === 0) {
              await this.db.exactConnection.update({
                where: { id: conn.id },
                data: { relationSyncCursor: cursor, relationSyncDone: processed },
              });
            }
          } catch (err: any) {
            this.logger.error(`Fout bij sync relatie ${relation.id}: ${err.message}`);
            await this.syncLog.log({
              entityType: 'relation',
              entityId: relation.id,
              action: 'create',
              status: 'failed',
              errorCode: String(err.exactStatus ?? ''),
              errorMessage: err.message,
              requestBody: this.buildAccountPayload(relation),
              triggeredBy,
            });
            // Ga door met volgende relatie — niet stoppen bij één fout
          }
        }
      }

      await this.db.exactConnection.update({
        where: { id: conn.id },
        data: {
          relationSyncStatus: 'done',
          relationSyncDone: processed,
          relationSyncCursor: null,
        },
      });

      this.logger.log(`Relatie sync voltooid: ${processed} relaties gesynchroniseerd`);
    } catch (err: any) {
      await this.db.exactConnection.update({
        where: { id: conn.id },
        data: {
          relationSyncStatus: 'failed',
          relationSyncDone: processed,
          relationSyncCursor: cursor,
        },
      });
      throw err;
    }
  }

  private buildAccountPayload(relation: {
    name: string;
    isSupplier: boolean;
    isCustomer: boolean;
    vatNumber?: string | null;
    email?: string | null;
    phone?: string | null;
    street?: string | null;
    houseNumber?: string | null;
    postalCode?: string | null;
    city?: string | null;
    country?: string | null;
    exactSupplierId?: string | null;
    exactCustomerId?: string | null;
  }) {
    // Gebruik de bestaande code als AccountCode in Exact (traceerbaarheid)
    const code = relation.exactSupplierId ?? relation.exactCustomerId ?? undefined;

    return {
      Name: relation.name,
      ...(code ? { Code: code } : {}),
      IsSupplier: relation.isSupplier,
      IsSales: relation.isCustomer,
      Email: relation.email ?? undefined,
      Phone: relation.phone ?? undefined,
      AddressLine1: relation.street
        ? `${relation.street} ${relation.houseNumber ?? ''}`.trim()
        : undefined,
      Postcode: relation.postalCode ?? undefined,
      City: relation.city ?? undefined,
      Country: relation.country ?? undefined,
      VATNumber: relation.vatNumber ?? undefined,
    };
  }

  // ---------------------------------------------------------------------------
  // SYNC ARTIKELEN → Exact Items
  // ---------------------------------------------------------------------------

  /**
   * Bulk-sync alle SUPWISE artikelen zonder exact_item_guid naar Exact.
   * Groepeert per unieke exact_item_code (1 Exact Item per code).
   * Artikelen zonder exact_item_code worden overgeslagen.
   */
  async syncAllArticles(triggeredBy: string): Promise<void> {
    const conn = await this.db.exactConnection.findFirst({ where: { isActive: true } });
    if (!conn) throw new Error('Geen actieve Exact koppeling');

    // Unieke codes zonder GUID
    const uniqueCodes: { exact_item_code: string }[] = await this.db.$queryRaw`
      SELECT DISTINCT exact_item_code
      FROM articles
      WHERE exact_item_guid IS NULL
        AND exact_item_code IS NOT NULL
        AND is_active = true
      ORDER BY exact_item_code
    `;

    const total = uniqueCodes.length;

    await this.db.exactConnection.update({
      where: { id: conn.id },
      data: { articleSyncStatus: 'running', articleSyncTotal: total, articleSyncDone: 0 },
    });

    let processed = 0;
    let lastCode = conn.articleSyncCursor ?? null;

    // Resume: begin pas na de opgeslagen cursor
    const startIdx = lastCode
      ? uniqueCodes.findIndex((c) => c.exact_item_code > lastCode) 
      : 0;

    try {
      for (let i = startIdx; i < uniqueCodes.length; i++) {
        const code = uniqueCodes[i].exact_item_code;

        // Haal één representatief artikel op voor de payload
        const article = await this.db.article.findFirst({
          where: { exactItemCode: code, isActive: true },
          select: {
            id: true,
            wsyArticleCode: true,
            description: true,
            unit: true,
            exactItemCode: true,
          },
        });

        if (!article) continue;

        try {
          const payload = this.buildItemPayload(article);
          const response = await this.api.post<ExactItemResponse>('/logistics/Items', payload);
          const guid = response.d.ID;

          // Update ALLE artikelen met dezelfde code
          await this.db.article.updateMany({
            where: { exactItemCode: code },
            data: { exactItemGuid: guid, exactItemSyncedAt: new Date() },
          });

          await this.syncLog.log({
            entityType: 'article',
            entityId: article.id,
            action: 'create',
            status: 'success',
            exactGuid: guid,
            requestBody: payload,
            responseBody: response.d,
            triggeredBy,
          });

          processed++;
          lastCode = code;

          if (processed % 10 === 0) {
            await this.db.exactConnection.update({
              where: { id: conn.id },
              data: { articleSyncCursor: lastCode, articleSyncDone: processed },
            });
          }
        } catch (err: any) {
          this.logger.error(`Fout bij sync artikel code ${code}: ${err.message}`);
          await this.syncLog.log({
            entityType: 'article',
            entityId: article.id,
            action: 'create',
            status: 'failed',
            errorCode: String(err.exactStatus ?? ''),
            errorMessage: err.message,
            requestBody: this.buildItemPayload(article),
            triggeredBy,
          });
        }
      }

      await this.db.exactConnection.update({
        where: { id: conn.id },
        data: {
          articleSyncStatus: 'done',
          articleSyncDone: processed,
          articleSyncCursor: null,
        },
      });

      this.logger.log(`Artikel sync voltooid: ${processed} unieke codes gesynchroniseerd`);
    } catch (err: any) {
      await this.db.exactConnection.update({
        where: { id: conn.id },
        data: {
          articleSyncStatus: 'failed',
          articleSyncDone: processed,
          articleSyncCursor: lastCode,
        },
      });
      throw err;
    }
  }

  private buildItemPayload(article: {
    wsyArticleCode: string;
    description: string;
    unit: string;
    exactItemCode: string | null;
  }) {
    return {
      Code: article.exactItemCode ?? article.wsyArticleCode,
      Description: article.description,
      IsSalesItem: true,
      IsPurchaseItem: true,
      Unit: this.mapUnit(article.unit),
    };
  }

  // SUPWISE unit → Exact unit code mapping
  private mapUnit(unit: string): string {
    const unitMap: Record<string, string> = {
      pc: 'Stuk',
      kg: 'kg',
      ltr: 'ltr',
      box: 'Doos',
      set: 'Set',
      m: 'm',
      m2: 'm2',
      m3: 'm3',
    };
    return unitMap[unit] ?? 'Stuk';
  }
}
```

---

### 4e. Order Push Service

**`exact-push.service.ts`:**

```typescript
import {
  Injectable,
  Logger,
  BadRequestException,
  NotFoundException,
} from '@nestjs/common';
import { DatabaseService } from '../../database/database.service';
import { ExactApiService } from './exact-api.service';
import { ExactSyncLogService } from './exact-sync-log.service';

interface PushResult {
  success: boolean;
  salesOrderGuid?: string;
  salesOrderNumber?: number;
  purchaseOrderResults: Array<{
    poId: string;
    poNumber: number;
    supplierId: string;
    supplierName: string;
    success: boolean;
    exactPoGuid?: string;
    error?: string;
  }>;
  errors: string[];
}

@Injectable()
export class ExactPushService {
  private readonly logger = new Logger(ExactPushService.name);

  constructor(
    private readonly db: DatabaseService,
    private readonly api: ExactApiService,
    private readonly syncLog: ExactSyncLogService,
  ) {}

  async pushOrder(orderId: string, triggeredBy: string): Promise<PushResult> {
    // ---------------------------------------------------------------------------
    // 1. Order ophalen met alle benodigde relaties
    // ---------------------------------------------------------------------------
    const order = await this.db.order.findUnique({
      where: { id: orderId },
      include: {
        customer: {
          select: {
            id: true,
            name: true,
            exactAccountGuid: true,
          },
        },
        lines: {
          where: { backorderStatus: 'none' },
          orderBy: { sortOrder: 'asc' },
          include: {
            article: {
              select: {
                id: true,
                wsyArticleCode: true,
                description: true,
                exactItemGuid: true,
                exactItemCode: true,
              },
            },
            supplier: {
              select: {
                id: true,
                name: true,
                exactAccountGuid: true,
              },
            },
          },
        },
        purchaseOrders: {
          include: {
            supplier: {
              select: {
                id: true,
                name: true,
                exactAccountGuid: true,
              },
            },
            lines: {
              include: {
                article: {
                  select: {
                    id: true,
                    exactItemGuid: true,
                    exactItemCode: true,
                    wsyArticleCode: true,
                  },
                },
              },
            },
          },
        },
      },
    });

    if (!order) throw new NotFoundException('Order niet gevonden');

    // ---------------------------------------------------------------------------
    // 2. Dependency check
    // ---------------------------------------------------------------------------
    const errors: string[] = [];

    if (!order.customer.exactAccountGuid) {
      errors.push(
        `Klant "${order.customer.name}" is nog niet gesynchroniseerd met Exact. ` +
        `Voer eerst de relatie-sync uit.`,
      );
    }

    for (const po of order.purchaseOrders) {
      if (!po.supplier.exactAccountGuid) {
        errors.push(
          `Leverancier "${po.supplier.name}" is nog niet gesynchroniseerd met Exact.`,
        );
      }
      for (const line of po.lines) {
        if (!line.article.exactItemGuid) {
          errors.push(
            `Artikel "${line.article.wsyArticleCode}" (${line.article.exactItemCode ?? 'geen code'}) ` +
            `is nog niet gesynchroniseerd met Exact.`,
          );
        }
      }
    }

    if (errors.length > 0) {
      throw new BadRequestException({
        message: 'Niet alle afhankelijkheden zijn gesynchroniseerd met Exact Online',
        errors,
      });
    }

    const result: PushResult = {
      success: false,
      purchaseOrderResults: [],
      errors: [],
    };

    // ---------------------------------------------------------------------------
    // 3. Sales Order aanmaken
    // ---------------------------------------------------------------------------
    const salesOrderPayload = {
      OrderedBy: order.customer.exactAccountGuid,
      YourRef: String(order.orderNumber),
      Description: order.description ?? `SUPWISE Order ${order.orderNumber}`,
      SalesOrderLines: order.lines.map((line) => ({
        Item: line.article!.exactItemGuid,
        Quantity: Number(line.quantity),
        UnitPrice: Number(line.unitSellPrice),
        Description: line.descriptionSnapshot,
      })),
    };

    try {
      const soResponse = await this.api.post<{ d: { OrderID: string; OrderNumber: number } }>(
        '/salesorder/SalesOrders',
        salesOrderPayload,
      );

      const soGuid = soResponse.d.OrderID;
      const soNumber = soResponse.d.OrderNumber;

      result.salesOrderGuid = soGuid;
      result.salesOrderNumber = soNumber;

      await this.db.order.update({
        where: { id: orderId },
        data: {
          exactSalesOrderId: soGuid,
          exactSalesOrderNumber: soNumber,
          exactSyncStatus: 'synced',
          exactLastSyncedAt: new Date(),
        },
      });

      await this.syncLog.log({
        entityType: 'sales_order',
        entityId: order.id,
        action: 'create',
        status: 'success',
        exactGuid: soGuid,
        requestBody: salesOrderPayload,
        responseBody: soResponse.d,
        triggeredBy,
      });
    } catch (err: any) {
      this.logger.error(`Sales Order push mislukt voor order ${orderId}: ${err.message}`);

      await this.db.order.update({
        where: { id: orderId },
        data: { exactSyncStatus: 'sync_failed' },
      });

      await this.syncLog.log({
        entityType: 'sales_order',
        entityId: order.id,
        action: 'create',
        status: 'failed',
        errorCode: String(err.exactStatus ?? ''),
        errorMessage: err.message,
        requestBody: salesOrderPayload,
        triggeredBy,
      });

      result.errors.push(`Sales Order aanmaken mislukt: ${err.message}`);
      return result;
    }

    // ---------------------------------------------------------------------------
    // 4. Purchase Orders aanmaken (per leverancier)
    // ---------------------------------------------------------------------------
    let allPoSuccess = true;

    for (const po of order.purchaseOrders) {
      const poPayload = {
        Supplier: po.supplier.exactAccountGuid,
        Description: `SUPWISE PO ${po.poNumber} - Order ${order.orderNumber}`,
        PurchaseOrderLines: po.lines.map((line) => ({
          Item: line.article.exactItemGuid,
          QuantityInPurchaseUnits: Number(line.quantity),
          NetPrice: Number(line.unitPurchasePrice),
          Description: line.descriptionSnapshot,
        })),
      };

      try {
        const poResponse = await this.api.post<{ d: { PurchaseOrderID: string } }>(
          '/purchaseorder/PurchaseOrders',
          poPayload,
        );

        const poGuid = poResponse.d.PurchaseOrderID;

        await this.db.purchaseOrder.update({
          where: { id: po.id },
          data: {
            exactPurchaseOrderId: poGuid,
            exactSyncStatus: 'synced',
            exactLastSyncedAt: new Date(),
          },
        });

        await this.syncLog.log({
          entityType: 'purchase_order',
          entityId: po.id,
          action: 'create',
          status: 'success',
          exactGuid: poGuid,
          requestBody: poPayload,
          responseBody: poResponse.d,
          triggeredBy,
        });

        result.purchaseOrderResults.push({
          poId: po.id,
          poNumber: po.poNumber,
          supplierId: po.supplier.id,
          supplierName: po.supplier.name,
          success: true,
          exactPoGuid: poGuid,
        });
      } catch (err: any) {
        allPoSuccess = false;
        this.logger.error(`PO push mislukt voor PO ${po.id}: ${err.message}`);

        await this.db.purchaseOrder.update({
          where: { id: po.id },
          data: { exactSyncStatus: 'sync_failed' },
        });

        await this.syncLog.log({
          entityType: 'purchase_order',
          entityId: po.id,
          action: 'create',
          status: 'failed',
          errorCode: String(err.exactStatus ?? ''),
          errorMessage: err.message,
          requestBody: poPayload,
          triggeredBy,
        });

        result.purchaseOrderResults.push({
          poId: po.id,
          poNumber: po.poNumber,
          supplierId: po.supplier.id,
          supplierName: po.supplier.name,
          success: false,
          error: err.message,
        });

        result.errors.push(
          `Inkooporder naar "${po.supplier.name}" (PO ${po.poNumber}) mislukt: ${err.message}`,
        );
      }
    }

    // ---------------------------------------------------------------------------
    // 5. Eindscore
    // ---------------------------------------------------------------------------
    result.success = allPoSuccess;

    // Als SO gelukt maar niet alle POs: order status = sync_failed (partieel)
    if (!allPoSuccess) {
      await this.db.order.update({
        where: { id: orderId },
        data: { exactSyncStatus: 'sync_failed' },
      });
    }

    return result;
  }
}
```

**Outdated-detectie:** Wanneer een order na een succesvolle push wordt bijgewerkt (status, regels, prijs), moet de `exact_sync_status` op `outdated` gezet worden. Dit gaat via een Prisma middleware of een trigger in de update-service van orders.

```typescript
// In orders.service.ts — toevoegen aan updateOrder():
if (order.exactSyncStatus === 'synced') {
  updateData.exactSyncStatus = 'outdated';
}
```

---

### 4f. Sync Log Service

**`exact-sync-log.service.ts`:**

```typescript
import { Injectable } from '@nestjs/common';
import { DatabaseService } from '../../database/database.service';

interface LogEntry {
  entityType: 'relation' | 'article' | 'sales_order' | 'purchase_order';
  entityId: string;
  action: 'create' | 'update' | 'delete';
  status: 'success' | 'failed' | 'retrying';
  attempt?: number;
  exactGuid?: string;
  errorCode?: string;
  errorMessage?: string;
  requestBody?: unknown;
  responseBody?: unknown;
  triggeredBy?: string;
}

interface SyncLogQuery {
  entityType?: string;
  entityId?: string;
  status?: string;
  from?: Date;
  to?: Date;
  limit?: number;
  cursor?: string;
}

@Injectable()
export class ExactSyncLogService {
  constructor(private readonly db: DatabaseService) {}

  async log(entry: LogEntry): Promise<void> {
    await this.db.exactSyncLog.create({
      data: {
        entityType: entry.entityType,
        entityId: entry.entityId,
        action: entry.action,
        status: entry.status,
        attempt: entry.attempt ?? 1,
        exactGuid: entry.exactGuid ?? null,
        errorCode: entry.errorCode ?? null,
        errorMessage: entry.errorMessage ?? null,
        requestBody: entry.requestBody ? (entry.requestBody as any) : undefined,
        responseBody: entry.responseBody ? (entry.responseBody as any) : undefined,
        triggeredBy: entry.triggeredBy ?? null,
      },
    });
  }

  async findAll(query: SyncLogQuery) {
    const { entityType, entityId, status, from, to, limit = 50, cursor } = query;

    const where: any = {};
    if (entityType) where.entityType = entityType;
    if (entityId) where.entityId = entityId;
    if (status) where.status = status;
    if (from || to) {
      where.createdAt = {};
      if (from) where.createdAt.gte = from;
      if (to) where.createdAt.lte = to;
    }

    const rows = await this.db.exactSyncLog.findMany({
      where,
      orderBy: { createdAt: 'desc' },
      take: limit + 1,
      ...(cursor ? { cursor: { id: cursor }, skip: 1 } : {}),
    });

    const hasMore = rows.length > limit;
    const page = hasMore ? rows.slice(0, limit) : rows;

    return {
      data: page,
      meta: {
        cursor: page.length > 0 ? page[page.length - 1]!.id : null,
        hasMore,
      },
    };
  }

  async getEntityHistory(entityType: string, entityId: string) {
    return this.db.exactSyncLog.findMany({
      where: { entityType, entityId },
      orderBy: { createdAt: 'desc' },
      take: 20,
    });
  }
}
```

---

## 5. API Endpoints

### Controller: `exact.controller.ts`

Alle endpoints onder prefix `/api/exact`. Vereist authenticatie. De meeste endpoints vereisen `admin` rol.

```typescript
@Controller('exact')
@UseGuards(AuthGuard, RolesGuard)
export class ExactController {
  constructor(
    private readonly auth: ExactAuthService,
    private readonly sync: ExactSyncService,
    private readonly push: ExactPushService,
    private readonly syncLog: ExactSyncLogService,
  ) {}
  // zie endpoints hieronder
}
```

### Endpoint-overzicht

| Method | Path | Rol | Beschrijving |
|--------|------|-----|--------------|
| `POST` | `/api/exact/connect` | admin | API key opslaan + connectie activeren |
| `GET` | `/api/exact/status` | admin | Connectiestatus, sync voortgang |
| `POST` | `/api/exact/sync/relations` | admin | Start bulk-sync relaties (async) |
| `POST` | `/api/exact/sync/articles` | admin | Start bulk-sync artikelen (async) |
| `GET` | `/api/exact/sync/progress` | admin | Sync voortgang (polling endpoint) |
| `POST` | `/api/exact/push/order/:id` | manager_senior, admin | Push order naar Exact |
| `GET` | `/api/exact/sync-log` | admin | Sync log ophalen (paginering) |
| `GET` | `/api/exact/sync-log/:entityType/:entityId` | admin | Log voor specifieke entiteit |

### Endpoint details

**`POST /api/exact/connect`**
```typescript
// Body: ConnectExactDto
// { apiKey: string, divisionId: number }
// Response: { connected: true, divisionId: number }
// Errors: 400 (ongeldige key), 403 (geen admin)
```

**`GET /api/exact/status`**
```typescript
// Response:
// {
//   connected: boolean,
//   divisionId?: number,
//   baseUrl?: string,
//   connectedAt?: string,
//   relationSync: {
//     status: 'pending' | 'running' | 'done' | 'failed',
//     total: number | null,
//     done: number,
//     pct: number,
//   },
//   articleSync: {
//     status: 'pending' | 'running' | 'done' | 'failed',
//     total: number | null,
//     done: number,
//     pct: number,
//   }
// }
```

**`POST /api/exact/sync/relations`**
```typescript
// Body: leeg
// Start syncAllRelations() als fire-and-forget (niet awaiten)
// Response: { started: true, message: 'Relatie sync gestart. Volg voortgang via /api/exact/sync/progress' }
// Errors: 400 als sync al loopt, 404 als geen connectie
```

**`POST /api/exact/sync/articles`**
```typescript
// Body: leeg
// Start syncAllArticles() als fire-and-forget
// Response: { started: true, message: 'Artikel sync gestart. Volg voortgang via /api/exact/sync/progress' }
```

**`GET /api/exact/sync/progress`**
```typescript
// Response: zelfde shape als /status (polling endpoint voor UI progress bars)
// Cache: 5 seconden
```

**`POST /api/exact/push/order/:id`**
```typescript
// Param: id (UUID)
// Body: leeg
// Response: PushOrderResponseDto {
//   success: boolean,
//   salesOrderGuid?: string,
//   salesOrderNumber?: number,
//   purchaseOrderResults: [...],
//   errors: string[],
// }
// Errors:
//   400 — afhankelijkheden missen (geeft lijst van ontbrekende GUIDs)
//   404 — order niet gevonden
//   409 — order al gesynchroniseerd (gebruik re-push als scope uitgebreid wordt)
```

**`GET /api/exact/sync-log`**
```typescript
// Query: SyncLogQueryDto
// { entityType?, entityId?, status?, from?, to?, limit?, cursor? }
// Response: { data: ExactSyncLog[], meta: { cursor, hasMore } }
```

**`GET /api/exact/sync-log/:entityType/:entityId`**
```typescript
// Response: laatste 20 log entries voor deze entiteit
```

### DTO's

**`connect-exact.dto.ts`:**
```typescript
import { IsString, IsInt, IsPositive, MinLength } from 'class-validator';

export class ConnectExactDto {
  @IsString()
  @MinLength(10)
  apiKey: string;

  @IsInt()
  @IsPositive()
  divisionId: number;
}
```

**`sync-log-query.dto.ts`:**
```typescript
export class SyncLogQueryDto {
  @IsOptional() @IsString() entityType?: string;
  @IsOptional() @IsUUID() entityId?: string;
  @IsOptional() @IsIn(['success','failed','retrying']) status?: string;
  @IsOptional() @IsISO8601() from?: string;
  @IsOptional() @IsISO8601() to?: string;
  @IsOptional() @IsInt() @Min(1) @Max(100) limit?: number;
  @IsOptional() @IsUUID() cursor?: string;
}
```

---

## 6. Frontend Wijzigingen

### 6.1 Admin pagina: Exact Koppeling

**Locatie:** `/admin/exact` (nieuwe pagina onder admin module)

**Componenten:**
- `ExactConnectionForm` — invoerveld voor API key + divisie ID, "Koppel Exact" knop
- `ExactConnectionStatus` — toont huidig verbindingsstatus badge (verbonden/niet verbonden)
- `SyncProgressCard` (×2) — relaties en artikelen, elk met progressbar + start/hervatten knop

**UI flow:**
1. Admin vult API key + divisie ID in
2. `POST /api/exact/connect` → succesbadge
3. Admin klikt "Start relatie sync" → `POST /api/exact/sync/relations`
4. UI pollt `GET /api/exact/sync/progress` elke 5 seconden → progressbar (`X / Y relaties`)
5. Na relatie sync: "Start artikel sync" knop actief
6. Zelfde flow voor artikelen
7. Na beide syncs klaar: "Klaar voor order push" melding

**Weergave sync voortgang:**
```
Relaties: [████████░░░░░░░░] 823 / 1.200 (68%)  ▶ Aan het synchroniseren...
Artikelen: [░░░░░░░░░░░░░░░░]   0 / 48.204 (0%)  ⏸ Niet gestart
```

### 6.2 Order detail: Push naar Exact

**Locatie:** bestaande order detail pagina — nieuwe sectie "Exact Online"

**Componenten:**
- `ExactSyncSection` — toon status badge + push knop
- `ExactPushModal` — bevestiging voor push + dependency check resultaat
- `ExactPoStatusTable` — tabel met per leverancier de PO sync-status

**UI states per order:**

| `exact_sync_status` | Badge | Knop |
|---------------------|-------|------|
| `not_synced` | ⬜ Niet gesynchroniseerd | "Push naar Exact" (blauw) |
| `synced` | ✅ Gesynchroniseerd | — (geen knop, of "Opnieuw pushen" grijs) |
| `sync_failed` | ❌ Synchronisatie mislukt | "Opnieuw proberen" (rood) |
| `outdated` | ⚠️ Verouderd | "Opnieuw pushen" (oranje) |

**ExactSyncSection layout:**
```
┌──────────────────────────────────────────────────────────┐
│ Exact Online                              [Push naar Exact▶] │
│─────────────────────────────────────────────────────────┤
│ Verkooporder: #EX-5012   ✅ Gesynchroniseerd             │
│ Laatste sync: 07-04-2026 14:32                           │
│                                                          │
│ Inkooporders:                                            │
│  Boltex Marine        PO-800045  ✅ Synced               │
│  MarineLED BV         PO-800046  ✅ Synced               │
│  TeakWorld            –          ❌ Mislukt  [Hervatten] │
└──────────────────────────────────────────────────────────┘
```

### 6.3 Orders overzicht: Exact kolom

Nieuwe kolom "Exact" in de orders tabel:

| Badge | `exact_sync_status` |
|-------|---------------------|
| — | `not_synced` |
| ✅ | `synced` |
| ⚠️ | `outdated` |
| ❌ | `sync_failed` |

### 6.4 Relaties en Artikelen: GUID indicatoren

**Relaties lijst** — extra kolom of badge:
- `exact_account_guid` gevuld → `✅ In Exact`
- `exact_account_guid` leeg → `⬜ Niet gesynchroniseerd`

**Artikelen lijst** — idem:
- `exact_item_guid` gevuld → `✅ In Exact`
- leeg → `⬜ Niet gesynchroniseerd`

Deze indicatoren geven admins inzicht in sync-voortgang en helpen bij troubleshooting.

---

## 7. Implementatievolgorde

Elke stap heeft een acceptatiecriterium (AC). Volg de volgorde — stap N+1 is afhankelijk van stap N.

**Stap 1 — Database migraties** *(~1 dag)*
- [ ] Schrijf SQL migraties (zie sectie 3)
- [ ] Voer uit op development Supabase
- [ ] AC: alle nieuwe kolommen en tabellen aanwezig, CHECK constraints werken, RLS policies actief

**Stap 2 — Prisma schema update** *(~2 uur)*
- [ ] Update `schema.prisma` met nieuwe modellen/velden (zie 3.8)
- [ ] `npx prisma generate`
- [ ] AC: TypeScript types compileren zonder fouten

**Stap 3 — Config schema + .env.example** *(~1 uur)*
- [ ] `config.schema.ts` uitbreiden (zie 4b)
- [ ] `.env.example` bijwerken
- [ ] Genereer `EXACT_ENCRYPTION_KEY`: `openssl rand -hex 32`
- [ ] AC: server start zonder crashen met nieuwe env vars

**Stap 4 — ExactModule scaffolding** *(~2 uur)*
- [ ] Map `apps/api/src/exact/` aanmaken
- [ ] `exact.module.ts` + lege service/controller files
- [ ] Importeer module in `app.module.ts`
- [ ] AC: server herstart succesvol, module geladen

**Stap 5 — ExactAuthService** *(~3 uur)*
- [ ] Implementeer encrypt/decrypt (AES-256-GCM)
- [ ] `saveConnection`, `getApiKey`, `isConnected`
- [ ] AC: unit test — opslaan en ophalen van API key levert dezelfde waarde

**Stap 6 — ExactApiService** *(~4 uur)*
- [ ] HTTP client wrapper
- [ ] Rate limiting (in-memory counter)
- [ ] Retry met exponential backoff
- [ ] AC: handmatige test tegen Exact testdivisie → `GET /api/v1/{div}/crm/Accounts` geeft 200

**Stap 7 — ExactSyncLogService** *(~2 uur)*
- [ ] `log()` en `findAll()` implementeren
- [ ] AC: log entry aanmaken en ophalen werkt

**Stap 8 — ExactSyncService: relaties** *(~1 dag)*
- [ ] `syncAllRelations()` implementeren
- [ ] `buildAccountPayload()` mapping
- [ ] AC: sync van 10 testrelaties → GUIDs opgeslagen in `exact_account_guid`

**Stap 9 — ExactSyncService: artikelen** *(~1 dag)*
- [ ] `syncAllArticles()` implementeren
- [ ] Deduplicatie per `exact_item_code`
- [ ] AC: sync van 100 testart artikelen → GUIDs opgeslagen, dubbele codes krijgen zelfde GUID

**Stap 10 — ExactPushService** *(~1 dag)*
- [ ] `pushOrder()` implementeren
- [ ] Dependency check
- [ ] Sales Order POST + response verwerking
- [ ] Purchase Orders POST per leverancier
- [ ] AC: push van testorder → 1 SO + N POs aangemaakt in testdivisie, GUIDs opgeslagen in DB

**Stap 11 — API Controller + Endpoints** *(~4 uur)*
- [ ] Alle endpoints implementeren (zie sectie 5)
- [ ] DTO validatie
- [ ] Rolbescherming
- [ ] AC: alle endpoints bereikbaar, juiste 403 op verkeerde rol

**Stap 12 — Outdated-detectie** *(~2 uur)*
- [ ] Middleware/hook in orders service bij update
- [ ] AC: order updaten na sync → `exact_sync_status` wordt `outdated`

**Stap 13 — Frontend: Admin connectiepagina** *(~1 dag)*
- [ ] `/admin/exact` pagina
- [ ] ConnectForm, SyncProgressCard
- [ ] Polling naar `/api/exact/sync/progress`
- [ ] AC: API key invullen → verbindingsstatus verschijnt, sync starten werkt

**Stap 14 — Frontend: Order push** *(~1 dag)*
- [ ] ExactSyncSection op order detail
- [ ] ExactPushModal met dependency check weergave
- [ ] ExactPoStatusTable
- [ ] AC: push knop zichtbaar, dependency fouten tonen, succes toont Exact ordernummers

**Stap 15 — Frontend: GUID indicatoren** *(~4 uur)*
- [ ] Kolom/badge op relaties lijst
- [ ] Kolom/badge op artikelen lijst
- [ ] AC: na sync toont de lijst ✅ badges voor gesynchroniseerde entiteiten

**Stap 16 — Integratie test + review** *(~1 dag)*
- [ ] End-to-end test: koppel → sync relaties → sync artikelen → push order
- [ ] Edge cases: ontbrekende GUID, rate limit hit, partieel succes
- [ ] AC: succesvol einde-tot-einde scenario doorlopen op testdivisie

---

## 8. Risico's en Open Vragen

### R1 — Duur van initiële artikel-sync (HOOG)

**Probleem:** ~200k artikelen, 60 req/min = ~56 uur bij normale rate limits. Ook bij hogere "bulk-import" limieten is dit uren werk.

**Mitigatie:**
- Sync draait als achtergrondproces, niet als HTTP request
- Resume-logica (cursor) zodat failures geen restart vereisen
- Vraag bij Exact na: is er een hogere limiet beschikbaar voor éénmalige bulk-import? (bijv. per request-ticket)
- Overweeg: parallelisatie (meerdere gelijktijdige requests), maar let op dat rate limits mogelijk per-IP of per-app gelden

**Open vraag:** Wat zijn de exacte rate limits voor de bulk-import periode?

### R2 — Niet-unieke `exact_item_code` (GEMIDDELD)

**Probleem:** Meerdere SUPWISE artikelen kunnen dezelfde `exact_item_code` delen. Als we één Exact Item aanmaken per unieke code, kunnen beschrijvingen conflicteren.

**Aanpak in dit plan:** De beschrijving van het eerste actieve artikel met die code wordt gebruikt.

**Open vraag:** Moet er handmatige review zijn voor artikelgroepen met meerdere SUPWISE artikelen op dezelfde code? Momenteel: nee, maar log-entry toont hoeveel artikelen per GUID.

### R3 — API key formaat Exact App Center (HOOG — vóór implementatie)

**Probleem:** De exacte authenticatiemethode voor privé Exact App Center apps moet bevestigd worden. De meeting zegt "API key", maar Exact's documentatie is onduidelijk over of dit een directe bearer token is, of een client_credentials OAuth2 grant, of iets anders.

**Actie vereist:** Verificeer met Exact API developer de exacte `Authorization` header format voor privé app-registraties vóór stap 6 (ExactApiService implementatie).

**Twee mogelijkheden:**
- Scenario A: Directe bearer token → implementeer zoals beschreven in 4c
- Scenario B: Client credentials grant → voeg token-exchange toe aan ExactAuthService (client_id + client_secret → short-lived access token via POST /oauth2/token)

### R4 — Verplichte velden bij POST naar Exact (GEMIDDELD)

**Probleem:** We weten niet precies welke velden verplicht zijn voor `POST /crm/Accounts` en `POST /logistics/Items`. Een ontbrekend verplicht veld geeft een 400 error.

**Mitigatie:** Test-sync met 5 relaties en 5 artikelen op testdivisie vóór bulk-sync. Log response bodies bij 400 errors.

**Open vraag:** Zijn BTW-nummer, adres, of eenheidscodes verplicht? Wat doet Exact als `Unit` niet matcht met een bekende eenheid?

### R5 — Eén Exact Account voor relatie die zowel klant als leverancier is (GEMIDDELD)

**Aanpak:** Dit plan gebruikt één `exact_account_guid` per relatie. Dit veronderstelt dat een Exact Account zowel debtor als creditor kan zijn via `IsSales: true` en `IsSupplier: true`.

**Open vraag:** Bevestig dit met Exact API developer. Als Exact aparte accounts vereist voor klant/leverancier, dan moeten we twee GUIDs opslaan (kolom `exact_customer_guid` naast `exact_supplier_guid`).

### R6 — Concurrency bij sync en order push (LAAG)

**Probleem:** De in-memory rate limiter in `ExactApiService` werkt alleen correct bij één server-instantie. Bij horizontale scaling (meerdere API pods) kunnen twee instances elk 60 req/min sturen → 120 req/min totaal.

**Mitigatie v1:** Aanname: één API-instantie. Als multi-instance nodig is, vervang in-memory counter door Redis-backed rate limiter (buiten scope v1).

### R7 — Outdated-detectie op order_lines (LAAG)

**Probleem:** Als een orderregel wordt gewijzigd (prijs, hoeveelheid), moet de order als `outdated` gemarkeerd worden. Dit vereist dat de orders service de Exact sync status reset.

**Aanpak:** Zie stap 12 — middleware in `orders.service.ts` bij `updateOrderLine`.

**Open vraag:** Wat is het re-push gedrag? V1 scope: geen re-push (gebruiker moet order opnieuw pushen = nieuwe Exact order aanmaken). Maar als SO al bestaat in Exact, leidt dit tot duplicaten. **Beslissing vereist vóór implementatie stap 10.**

### R8 — Relaties die niet in Exact testdivisie bestaan maar wél een CSV code hebben

**Probleem:** `exact_supplier_id` / `exact_customer_id` zijn gevuld met CSV codes (bijv. "20180"). Dat zijn GEEN Exact GUIDs. Na de sync worden nieuwe Exact accounts aangemaakt. Als de echte productiedivisie WEL deze relaties kent onder die codes, kan de bulk-sync duplicaten aanmaken.

**Actie vereist:** Verifieer met klant: zijn de relaties in de productiedivisie aanwezig? Zo ja, dan moeten we in de sync EERST proberen te zoeken op code (`GET /crm/Accounts?$filter=Code eq '20180'`) en de GUID ophalen, en pas POSTen als niet gevonden. Dit is een aanzienlijke uitbreiding van `syncAllRelations()`.

**Voor testdivisie:** dit risico speelt niet (alles leeg → altijd POST).

---

## Bijlage A — Minimum Viable Payload per endpoint

### `POST /crm/Accounts` (relatie)
```json
{
  "Name": "Boltex Marine BV",
  "Code": "20180",
  "IsSupplier": true,
  "IsSales": false,
  "Email": "inkoop@boltex.nl",
  "VATNumber": "NL123456789B01",
  "AddressLine1": "Industrieweg 42",
  "Postcode": "3077 AW",
  "City": "Rotterdam",
  "Country": "NL"
}
```
**Response:** `{ "d": { "ID": "a1b2c3d4-...", "Code": "20180", "Name": "Boltex Marine BV" } }`

### `POST /logistics/Items` (artikel)
```json
{
  "Code": "8701003844",
  "Description": "Marine LED Lamp 12V 5W",
  "IsSalesItem": true,
  "IsPurchaseItem": true,
  "Unit": "Stuk"
}
```
**Response:** `{ "d": { "ID": "e5f6a7b8-...", "Code": "8701003844" } }`

### `POST /salesorder/SalesOrders`
```json
{
  "OrderedBy": "a1b2c3d4-1234-...",
  "YourRef": "700048",
  "Description": "SUPWISE Order 700048",
  "SalesOrderLines": [
    {
      "Item": "e5f6a7b8-5678-...",
      "Quantity": 10,
      "UnitPrice": 1.67,
      "Description": "Marine LED Lamp 12V 5W"
    }
  ]
}
```
**Response:** `{ "d": { "OrderID": "f9g0h1i2-...", "OrderNumber": 5012 } }`

### `POST /purchaseorder/PurchaseOrders`
```json
{
  "Supplier": "b2c3d4e5-...",
  "Description": "SUPWISE PO 800045 - Order 700048",
  "PurchaseOrderLines": [
    {
      "Item": "e5f6a7b8-5678-...",
      "QuantityInPurchaseUnits": 10,
      "NetPrice": 1.45,
      "Description": "Marine LED Lamp 12V 5W"
    }
  ]
}
```
**Response:** `{ "d": { "PurchaseOrderID": "j3k4l5m6-..." } }`

---

## Bijlage B — .env.example aanvullende entries

```bash
# Exact Online (Phase 5B — API key authenticatie, GEEN OAuth2)
# Verwijder: EXACT_CLIENT_ID, EXACT_CLIENT_SECRET, EXACT_REDIRECT_URI
# Voeg toe:
EXACT_ENCRYPTION_KEY=    # Genereer: openssl rand -hex 32
EXACT_BASE_URL=https://start.exactonline.nl
EXACT_SYNC_BATCH_SIZE=10
# De API key zelf wordt NIET in .env opgeslagen — alleen encrypted in DB via POST /api/exact/connect
```

---

*Einde implementatieplan. Vragen of onduidelijkheden: raadpleeg de risico's sectie (§8) voor actiepunten die vóór implementatie beantwoord moeten worden.*
