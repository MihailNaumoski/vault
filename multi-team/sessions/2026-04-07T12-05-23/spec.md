# Phase 5B — Exact Online Integration: Acceptance Criteria

**Date:** 2026-04-07
**Status:** FINAL
**Auth:** API key (confirmed by Exact Online meeting, NOT OAuth2)

---

## Feature 1: Connection Management

Admin connects SUPWISE to Exact Online via API key.

### Happy Path

**AC-1.1:** Admin navigates to `/admin/exact`, enters an API key and division ID, and clicks "Connect". The system encrypts the API key with AES-256-GCM, stores it in `exact_connections` with `is_active = true`, and calls `GET /api/v1/{division}/current/Me` to verify the connection. On success, the page displays "Connected" with the division ID and connected timestamp.

**AC-1.2:** Only one connection may be active at any time. When a new connection is created, any previously active connection is set to `is_active = false` before the new one is inserted.

**AC-1.3:** The API key is never stored in plaintext. The `api_key_enc` column contains the AES-256-GCM ciphertext, and the `encryption_iv` column contains the initialization vector. The auth tag is appended to the ciphertext.

**AC-1.4:** Admin clicks "Disconnect". The active connection is set to `is_active = false`. All Exact-related actions (sync, push) become unavailable until a new connection is established.

**AC-1.5:** The connection status page (`/admin/exact`) shows: connection status (active/disconnected), division ID, connected by (username), connected at (timestamp), and last successful sync timestamps for relations and articles.

### Error Scenarios

**AC-1.6:** If the verification call to `/current/Me` fails (401, 403, network error), the connection is NOT saved. The UI displays the error message: "Could not connect to Exact Online. Verify your API key and division ID."

**AC-1.7:** If `EXACT_ENCRYPTION_KEY` environment variable is missing or invalid (not 64 hex chars), the connect endpoint returns 500 with a clear error logged server-side. The admin sees "Configuration error — contact system administrator."

**AC-1.8:** If the admin attempts to sync or push without an active connection, all Exact endpoints return 401 with message "No active Exact Online connection."

---

## Feature 2: Bulk Sync — Relations to Accounts

Admin syncs all SUPWISE relations to Exact Online as Accounts.

### Happy Path

**AC-2.1:** Admin clicks "Sync Relations" on `/admin/exact/sync`. The system queries all active relations where `exact_account_guid IS NULL` and at least one of `exact_supplier_id` or `exact_customer_id` is not null. For each relation, it sends `POST /crm/Accounts` with the relation data and stores the returned GUID in `relations.exact_account_guid`.

**AC-2.2:** The Account payload includes: `Code` (from `exact_supplier_id` or `exact_customer_id`), `Name`, `IsPurchase` (from `is_supplier`), `IsSales` (from `is_customer`), `Country`, `City`, `Postcode`, `Phone`, `Email`, `VATNumber`.

**AC-2.3:** After successful sync, `relations.exact_synced_at` is set to the current timestamp.

**AC-2.4:** A relation that already has an `exact_account_guid` is skipped on re-sync (idempotent behavior).

**AC-2.5:** Every sync attempt (success or failure) is logged in `exact_sync_log` with `entity_type = 'account'`, the entity ID, action, status, request body, and response body.

**AC-2.6:** The sync progress is visible in the UI: total relations to sync, number synced, number failed, and a list of errors with relation name and error message.

### Error Scenarios

**AC-2.7:** If a single relation fails (400 from Exact), the sync continues with the next relation. The failed relation is logged in `exact_sync_log` with `status = 'failed'` and the error message. The progress counter increments `failed`.

**AC-2.8:** If the circuit breaker triggers (7 errors on the `/crm/Accounts` endpoint), the sync halts immediately. The UI displays: "Sync paused: too many errors. {N} of {total} relations synced. Review errors and retry." The remaining relations are NOT attempted.

**AC-2.9:** If a 429 (rate limit) response is received, the sync pauses automatically. It waits until the rate limit window resets (using `X-RateLimit-Minutely-Reset` or `Retry-After` header) and then resumes.

**AC-2.10:** Relations where both `exact_supplier_id` and `exact_customer_id` are null are excluded from sync. These relations have no Exact code to use as the Account `Code`.

---

## Feature 3: Bulk Sync — Articles to Items

Admin syncs all SUPWISE articles to Exact Online as Items, with deduplication.

### Happy Path

**AC-3.1:** Admin clicks "Sync Articles" on `/admin/exact/sync`. The system queries all active articles where `exact_item_guid IS NULL` and `exact_item_code IS NOT NULL`. Articles are grouped by `exact_item_code` — only one POST per unique code.

**AC-3.2:** For each unique `exact_item_code`, the system sends `POST /logistics/Items` with: `Code`, `Description` (from the first article in the group), `IsSalesItem = true`, `IsPurchaseItem = true`, `Unit` (mapped to Exact unit code), and optional `Barcode` (EAN) and `NetWeight`.

**AC-3.3:** The returned GUID is stored in `exact_item_guid` for ALL articles sharing that `exact_item_code`, not just the first one.

**AC-3.4:** After successful sync for a code group, `articles.exact_synced_at` is set for all articles in the group.

**AC-3.5:** Articles that already have an `exact_item_guid` are skipped on re-sync (idempotent).

**AC-3.6:** The sync progress UI shows: total unique codes to sync, number synced, number failed, and errors with the article code and error message.

**AC-3.7:** Every sync attempt is logged in `exact_sync_log` with `entity_type = 'item'`.

### Error Scenarios

**AC-3.8:** If a single code group fails, the sync continues with the next code. The failed group is logged. All articles in the failed group retain `exact_item_guid = NULL`.

**AC-3.9:** Circuit breaker: at 7 errors on `/logistics/Items`, the sync halts. The UI shows how many unique codes were synced vs. total, and lists all errors.

**AC-3.10:** Rate limit handling: if minutely limit is approached (55 of 60), the sync pauses until the next minute. If daily limit is approached (4,950 of 5,000), the sync pauses and shows: "Daily API limit nearly reached. Sync will resume tomorrow. {N}/{total} synced."

**AC-3.11:** The sync is resumable: if interrupted (server restart, daily limit reached), re-running sync only processes articles without an `exact_item_guid`. Progress is not lost.

**AC-3.12:** Articles with `exact_item_code = NULL` are excluded from sync with a warning count: "{N} articles have no Exact item code and were skipped."

---

## Feature 4: Order Push — Sales Order

Manual "Push to Exact" creates a Sales Order in Exact for a SUPWISE order.

### Happy Path

**AC-4.1:** User clicks "Push to Exact" on an order detail page. The system first runs a dependency check: customer `exact_account_guid` exists, all order line articles have `exact_item_guid`, all suppliers on order lines have `exact_account_guid`.

**AC-4.2:** If all dependencies are met, the system sends `POST /salesorder/SalesOrders` with `OrderedBy` = customer GUID, and `SalesOrderLines` containing each order line with `Item` = article GUID, `Quantity`, `NetPrice` = unit sell price, and `Description`.

**AC-4.3:** On success, the returned `OrderID` (GUID) is stored in `orders.exact_sales_order_id`, the `OrderNumber` in `orders.exact_sales_order_number`, and each line's `ID` in `order_lines.exact_sales_line_id`.

**AC-4.4:** The order's `exact_sync_status` is set to `synced` and `exact_last_synced_at` to the current timestamp.

**AC-4.5:** A sync log entry is created with `entity_type = 'sales_order'`, `action = 'create'`, `status = 'success'`.

### Error Scenarios

**AC-4.6:** If the dependency check fails, the push is NOT attempted. The UI shows a checklist:
- "Customer [name] has no Exact Account GUID — sync relations first"
- "Article [code] has no Exact Item GUID — sync articles first"
- "Supplier [name] has no Exact Account GUID — sync relations first"

Each missing item is listed individually.

**AC-4.7:** If the Sales Order POST fails (400, 500, timeout), the order's `exact_sync_status` is set to `sync_failed`. The error is logged in `exact_sync_log`. The UI shows the error message and a "Retry" button.

**AC-4.8:** If the order has already been pushed (`exact_sales_order_id` is not null, `exact_sync_status = 'synced'`), the "Push" button is disabled. The UI shows: "Already synced — Exact SO #{number}".

**AC-4.9:** If the order's status is `outdated` (modified after sync), the Push button shows "Re-push to Exact" and is enabled. A re-push creates a new Sales Order (not an update). The old SO reference is overwritten.

---

## Feature 5: Order Push — Purchase Orders

After Sales Order push, Purchase Orders are created per supplier.

### Happy Path

**AC-5.1:** Immediately after successful Sales Order creation, the system groups the order's purchase order records by supplier. For each `purchase_orders` record, it sends `POST /purchaseorder/PurchaseOrders` with `Supplier` = supplier GUID, `ReceiptDate` = order load date, and `PurchaseOrderLines` with `Item` = article GUID, `QuantityInPurchaseUnits`, `NetPrice` = unit purchase price.

**AC-5.2:** On success, the returned `PurchaseOrderID` is stored in `purchase_orders.exact_purchase_order_id`, `PurchaseOrderNumber` in `purchase_orders.exact_purchase_order_number`, and each line's ID in `purchase_order_lines.exact_po_line_id`.

**AC-5.3:** Each purchase order's `exact_sync_status` is set to `synced` and `exact_last_synced_at` to the current timestamp.

**AC-5.4:** A sync log entry is created per purchase order with `entity_type = 'purchase_order'`, `action = 'create'`, `status = 'success'`.

### Partial Failure

**AC-5.5:** If the Sales Order succeeds but one or more Purchase Orders fail, the order-level `exact_sync_status` remains `sync_failed`. The successfully pushed POs have `exact_sync_status = 'synced'`. The failed POs have `exact_sync_status = 'sync_failed'`.

**AC-5.6:** The UI on the order detail page shows per-PO status:
```
Inkooporders:
| Supplier     | Exact PO #    | Status    |
| Boltex       | EX-PO-1234    | Synced    |
| MarineLED    | EX-PO-1235    | Synced    |
| TeakWorld    | —             | Failed    |
```
Failed POs show a "Retry" button that pushes only that specific PO.

**AC-5.7:** Individual PO retry: `POST /api/exact/push/purchase-order/:id` re-attempts the push for a single failed purchase order. On success, updates the PO's sync status. If all POs are now synced, the order-level status is updated to `synced`.

### Error Scenarios

**AC-5.8:** If a Purchase Order POST fails, the error is logged in `exact_sync_log` with the request body, response body, and error message. The PO's `exact_sync_status` is set to `sync_failed`.

**AC-5.9:** Purchase orders that have not yet been created in SUPWISE (no `purchase_orders` records for this order) cannot be pushed. The UI shows: "Generate purchase orders first before pushing to Exact."

---

## Feature 6: Sync Status Tracking

Visual status indicators across the application.

### Order Overview List

**AC-6.1:** The order overview list (`/orders`) has an "Exact" column showing a status badge:
| Badge | Condition |
|-------|-----------|
| — (dash) | `exact_sync_status = 'not_synced'` |
| Synced (green) | `exact_sync_status = 'synced'` |
| Outdated (orange) | `exact_sync_status = 'outdated'` |
| Failed (red) | `exact_sync_status = 'sync_failed'` |

**AC-6.2:** The Synced badge shows the Exact Sales Order number on hover: "Exact SO #12345".

### Order Detail Page — Exact Section

**AC-6.3:** Below the order lines, a new "Exact Online" section shows:
- Connection status
- Sales Order: Exact SO number + sync status + last synced timestamp
- Purchase Orders table: supplier, Exact PO number, sync status, line count
- "Push to Exact" button (or retry, or disabled if already synced)

**AC-6.4:** If any PO has `sync_failed`, the section shows a warning: "{N} purchase order(s) could not be synced." with a "Retry failed" button.

### Purchase Order Overview

**AC-6.5:** The PO overview list (`/purchase-orders`) has an "Exact" column showing the same badge pattern: dash, Synced, Failed.

### Purchase Order Detail

**AC-6.6:** The PO detail page shows an "Exact Online" section with: Exact PO number, sync status, last synced timestamp, and "Push to Exact" / "Retry" button.

### Outdated Detection

**AC-6.7:** When an order with `exact_sync_status = 'synced'` is modified (description, reference, load_date, customer_id, default_margin_pct, currency, or delivery_method changes), a database trigger automatically sets `exact_sync_status = 'outdated'`.

**AC-6.8:** An "outdated" order shows an orange badge and message: "Changes not synced to Exact. Push again to update."

---

## Feature 7: Sync Audit Log

Complete audit trail of all Exact API interactions.

### Happy Path

**AC-7.1:** Every Exact API call (success or failure) creates an entry in `exact_sync_log` with: entity type, entity ID, action (create/update/delete), status (success/failed/retrying), attempt number, request body (sanitized — no API key), response body, timestamp, and user who triggered it.

**AC-7.2:** The sync log is viewable at `/admin/exact/log` (admin only). It shows a paginated, filterable list with columns: timestamp, entity type, entity ID (linked to the SUPWISE entity), action, status, and error message (if failed).

**AC-7.3:** Filters available: entity type (account, item, sales_order, purchase_order), status (success, failed), date range.

**AC-7.4:** Clicking a log entry shows full detail: the complete request body and response body (as formatted JSON).

### Error Scenarios

**AC-7.5:** The request body stored in the log NEVER contains the API key. Only the Exact endpoint path and payload data are stored.

**AC-7.6:** If the sync log insert itself fails (database error), it does not prevent the sync from continuing. The failure is logged to the application logger (stdout/stderr).

---

## Feature 8: Rate Limiting and Error Handling

Protection against Exact Online API limits.

### Rate Limiting

**AC-8.1:** The system monitors both `X-RateLimit-Remaining` (daily) and `X-RateLimit-Minutely-Remaining` (minutely) headers from every Exact API response. When Exact switches to minutely headers (approaching minutely limit), the system respects the minutely limit.

**AC-8.2:** Pre-emptive throttling: the system pauses requests when minutely remaining drops to 5 (55 of 60 used), waiting until the minutely reset time.

**AC-8.3:** Daily limit protection: the system stops all sync operations when daily remaining drops to 50 (4,950 of 5,000 used). Sync resumes after midnight (Exact's timezone reset).

**AC-8.4:** For transient errors (429, 408, 503), the system retries with exponential backoff: 1 second, 4 seconds, 16 seconds. Maximum 3 retries per call.

### Circuit Breaker

**AC-8.5:** The circuit breaker tracks errors per endpoint per hour. At 7 errors (safety margin before Exact's hard limit of 10), all requests to that endpoint are halted for 1 hour.

**AC-8.6:** When the circuit breaker activates during bulk sync, the sync pauses and reports: "Too many errors on [endpoint]. Sync paused for 1 hour. Review the {N} errors in the sync log."

### Pre-flight Validation

**AC-8.7:** Before every API call, the payload is validated locally:
- Account: `Code` and `Name` are non-empty, `Name` is max 50 characters
- Item: `Code` and `Description` are non-empty, `Description` is max 60 characters
- Sales Order: `OrderedBy` is a valid GUID, all line `Item` values are valid GUIDs
- Purchase Order: `Supplier` is a valid GUID, all line `Item` values are valid GUIDs

**AC-8.8:** Records that fail pre-flight validation are NOT sent to the API. They are logged in `exact_sync_log` with `status = 'failed'` and a descriptive error (e.g., "Name exceeds 50 characters"). They do NOT count toward the error velocity limit.

### Non-retriable Errors

**AC-8.9:** HTTP 400, 401, 403, 404 responses are NOT retried. They are immediately logged as failed. 401 specifically triggers a connection status check — if the API key is invalid, the admin is notified.

---

## Feature 9: Admin Sync Page

Dedicated admin page for managing Exact Online sync operations.

**AC-9.1:** The admin page at `/admin/exact/sync` has three sections: Connection Status, Relation Sync, Article Sync.

**AC-9.2:** Connection Status section shows: connected (yes/no), division ID, connected by, last verified.

**AC-9.3:** Relation Sync section shows: total relations with Exact codes, number already synced (have GUID), number pending. A "Sync Relations" button starts the sync.

**AC-9.4:** Article Sync section shows: total unique `exact_item_code` values, number already synced, number pending. A "Sync Articles" button starts the sync.

**AC-9.5:** During sync, both sections show a progress bar with: "{synced}/{total} synced, {failed} failed" updating in real-time (SSE or polling).

**AC-9.6:** After sync completes (or is halted by circuit breaker), a summary is shown with the option to download the error list.

**AC-9.7:** A "Trial Run" option syncs only the first 5 records (2 relations, 3 articles) to verify the connection and payload format before running the full sync.

---

## Cross-Cutting Concerns

**AC-CC.1:** All Exact-related admin pages and endpoints require the `admin` role. The "Push to Exact" button on order detail is available to `admin` and order handler roles (`ob_senior`, `ob_junior`).

**AC-CC.2:** The `exact_connections.api_key_enc` column is never returned by any API endpoint. The admin page shows only the connection status, never the key itself.

**AC-CC.3:** All Exact API calls use `Authorization: Bearer {decrypted_api_key}` header. If the header format needs to differ (discovered during verification), update `exact-api.service.ts` accordingly.

**AC-CC.4:** The Exact module has no dependency on BullMQ, Redis, or WebSockets. All sync operations are synchronous HTTP request loops with in-memory rate limiting. This is sufficient for v1 given the relatively small dataset (~200 relations).

**AC-CC.5:** All database changes follow the established pattern: Supabase migration creates the SQL, Prisma schema is updated manually, `prisma generate` produces the typed client.
