# Exact Online API — SDK & Code Analysis Findings

**Date**: 2026-04-07  
**Analyst**: SDK Analyst (Research Team)

**Sources analyzed**:
1. **`exact-online` npm v0.1.5** — Node.js wrapper by jellekralt (`https://github.com/AanZee/node-exact-online`)
2. **`@quantix-ict/exact-online` npm v1.1.0** — TypeScript client by Quantix ICT
3. **`n8n-nodes-exact-online` npm v0.2.5** — n8n community integration (bramknuever)
4. **`@datafix/n8n-nodes-exact-online` npm v0.3.7** — Improved n8n integration (datafix)
5. **`ossobv/exactonline` Python v49⭐** — Most popular Python SDK (Walter Doekes, OSSO B.V.)
6. **`alexander-schillemans/python-exact-online` Python v5⭐** — Simpler Python wrapper
7. **`mcnijman/go-exactonline` Go v7⭐** — Full Go SDK with auto-generated types
8. **`DannyvdSluijs/ExactOnlineRestApiReference`** — Machine-readable API metadata (5.8MB JSON)

---

## A. OAuth2 Implementation

### A1. Token Refresh Implementation

Every SDK confirms: **OAuth2 Authorization Code flow only. No daemon/service-account mode exists.**

#### Node.js (`exact-online` npm) — Token Refresh
**File**: `lib/exactonline.js`, `refreshToken()` method
```javascript
// Token refresh — content-type is application/x-www-form-urlencoded (NOT JSON)
Client.prototype.token = function(code, grantType, redirectUri, callback) {
  var data = {
    grant_type: grantType,       // 'refresh_token'
    client_id: this.options.clientId,
    client_secret: this.options.clientSecret,
  }
  switch(grantType) {
    case 'refresh_token':
      data.refresh_token = code;
      break;
  }
  // Content-Type: application/x-www-form-urlencoded
  this.sendRequest('/oauth2/token', 'POST', {}, data, callback);
};
```

#### Quantix ICT (`@quantix-ict/exact-online`) — Token Refresh  
**File**: `dist/cjs/index.js`, `refreshTokens()` method
```javascript
const payload = {
    refresh_token: refreshToken,
    grant_type: 'refresh_token',
    client_id: this.clientId,
    client_secret: this.clientSecret,
};
const res = await fetch('https://start.exactonline.nl/api/oauth2/token', {
    method: 'POST',
    headers: {
        'Content-Type': 'application/x-www-form-urlencoded;charset=UTF-8',
        Accept: 'application/json',
    },
    body: new URLSearchParams(payload).toString(),
});
const data = await res.json();
this.setAccessToken(data.access_token);
this.setRefreshToken(data.refresh_token);  // ← NEW refresh token saved
```

#### Python (`ossobv/exactonline`) — Token Refresh
**File**: `exactonline/rawapi.py`, `refresh_token()` method
```python
refresh_params = {
    'client_id': binquote(self.storage.get_client_id()),
    'client_secret': binquote(self.storage.get_client_secret()),
    'grant_type': 'refresh_token',
    'refresh_token': binquote(self.storage.get_refresh_token()),
}
refresh_data = ('client_id=%(client_id)s'
                '&client_secret=%(client_secret)s'
                '&grant_type=%(grant_type)s'
                '&refresh_token=%(refresh_token)s' % refresh_params)
url = self.storage.get_refresh_url()  # Same as token_url
response = http_req('POST', url, refresh_data, opt=opt_secure, limiter=self.limiter)
```

### A2. Exact HTTP Request for Token Refresh

**Confirmed across ALL SDKs:**
```http
POST https://start.exactonline.nl/api/oauth2/token
Content-Type: application/x-www-form-urlencoded

grant_type=refresh_token&client_id={CLIENT_ID}&client_secret={CLIENT_SECRET}&refresh_token={REFRESH_TOKEN}
```

**Response:**
```json
{
  "access_token": "AAEA...",
  "token_type": "bearer",
  "expires_in": "600",
  "refresh_token": "__1P!I..."
}
```

**CRITICAL**: Content-Type MUST be `application/x-www-form-urlencoded`, NOT `application/json`. Body is form-encoded, NOT JSON. All three SDKs confirmed this independently.

### A3. Automatic Token Refresh on 401

| SDK | Pattern | Source |
|-----|---------|--------|
| `exact-online` npm | `checkAuth()` called before every request — refreshes if `expires < Date.now()` | `lib/exactonline.js` |
| `@quantix-ict/exact-online` | Checks `res.status === 401` → calls `refreshTokens()` → retries request | `dist/cjs/index.js` |
| `ossobv/exactonline` Python | Proactive refresh 30s before expiry via `Autorefresh` mixin. Falls back to retry on 401 if token still rejected | `exactonline/api/autorefresh.py` |
| `alexander-schillemans` Python | Checks `isTokenDueRenewal()` before every request (30s buffer) | `exactonline/authhandler.py` |
| n8n integration | Delegates to n8n's `requestWithAuthentication` (handles OAuth2 refresh internally) | `GenericFunctions.js` |

**Best practice (from Python SDK docs quoted in code):**
> "Do not request a new access token too late" — refresh before expiry  
> "Do not request a new access token too early" — only after 9 min 30 sec  
> Recommended: refresh when < 30 seconds remain

### A4. Daemon/Unattended Access — CONFIRMED IMPOSSIBLE

**No SDK implements a daemon mode.** All require an initial browser-based authorization code flow.

The pattern for server-side unattended access is:
1. **One-time manual setup**: Admin completes OAuth2 browser flow, gets authorization code
2. **Exchange code for tokens**: Server stores access_token + refresh_token
3. **Automatic refresh loop**: Server refreshes token before every ~10 min expiry, getting a NEW refresh_token each time
4. **Persist new refresh_token**: After every refresh, the new refresh_token must be saved (the old one is invalidated)

**WARNING from Quantix ICT SDK** (`dist/cjs/index.js`):
```javascript
// Uses a `refreshing` mutex flag to prevent concurrent refresh requests
if (this.refreshing) {
    await sleep(1000);
    return;
}
this.refreshing = true;
```
This is necessary because the refresh token is **single-use** — if two requests try to refresh simultaneously, the second will fail because the first already consumed the refresh token.

### A5. Refresh Token Rotation Handling

**All SDKs save the new refresh token after every refresh.** This is mandatory because:
- Each refresh returns a NEW refresh_token
- The old refresh_token is immediately invalidated
- If you fail to save the new refresh_token, you lose access permanently (must re-authenticate via browser)

| SDK | Storage Method |
|-----|----------------|
| `exact-online` npm | In-memory only (`this.oauth.refreshToken = token.refresh_token`) |
| `@quantix-ict/exact-online` | File system: `os.tmpdir()/quantix-ict-exact/refresh.json` |
| `ossobv/exactonline` Python | Configurable via `storage` backend (INI file, database, etc.) |
| `alexander-schillemans` Python | File system: `cache/{clientId}.txt` as JSON |

### A6. Token Storage Patterns

#### Quantix ICT — File-based  
**File**: `dist/cjs/index.js`
```javascript
setRefreshToken(refreshToken) {
    const tempDir = path.join(os.tmpdir(), 'quantix-ict-exact');
    if (!fs.existsSync(tempDir)) fs.mkdirSync(tempDir);
    fs.writeFileSync(
        path.join(tempDir, 'refresh.json'),
        JSON.stringify({ token: refreshToken })
    );
}
```

#### Python ossobv — INI file
**File**: `exactonline/storage/ini_example.ini`
```ini
[server]
auth_url = https://start.exactonline.nl/api/oauth2/auth
rest_url = https://start.exactonline.nl/api
token_url = https://start.exactonline.nl/api/oauth2/token

[application]
base_url = https://example.com
client_id = {12345678-abcd-1234-abcd-0123456789ab}
client_secret = ZZZ999xxx000

[transient]
access_expiry = 1426492503
access_token = dAfjGhB1k2tE2dkG12sd1Ff1A1fj2fH2Y1j1fKJl2f1sD1ON275zJNUy...
code = dAfj!hB1k2tE2dkG12sd1Ff1A1fj2fH2Y1j1fKJl2f1sDfKJl2f1sD11FfUn1...
division = 123456
refresh_token = SDFu!12SAah-un-56su-1fj2fH2Y1j1fKJl2f1sDfKJl2f1sD11FfUn1...
```

**For SUPWISE's NestJS backend**: Use a database table to store tokens. Must support atomic read-write to prevent concurrent refresh issues.

---

## B. GUID Resolution

### B1. Account Lookup by Code

#### CRITICAL: Account Code Padding (18 chars, leading spaces)

**Confirmed in API metadata** (`DannyvdSluijs/ExactOnlineRestApiReference`):
> Account.Code: "Unique key, **fixed length numeric string with leading spaces, length 18**. IMPORTANT: When you use OData $filter on this field you have to make sure the filter parameter contains the leading spaces"

**Python SDK implementation** (`exactonline/api/relations.py`):
```python
class Relations(Manager):
    resource = 'crm/Accounts'

    def filter(self, relation_code=None, **kwargs):
        if relation_code is not None:
            remote_id = self._remote_relation_code(relation_code)
            self._filter_append(kwargs, u'Code eq %s' % (remote_id,))
        return super(Relations, self).filter(**kwargs)

    def _remote_relation_code(self, code):
        return u"'%18s'" % (code.replace("'", "''"),)
        #       ^^^^^ — left-pad with spaces to 18 chars!
```

**Resulting OData query:**
```
GET /api/v1/{division}/crm/Accounts?$filter=Code eq '           1234567'&$select=ID,Code,Name
```
Where `1234567` is right-aligned in an 18-character string padded with leading spaces.

### B2. Item Lookup by Code

**Python SDK implementation** (`exactonline/api/items.py`):
```python
class Items(Manager):
    resource = 'logistics/Items'

    def filter(self, code=None, **kwargs):
        if 'select' not in kwargs:
            kwargs['select'] = 'ID,Code,CostPriceStandard,Description'
        if code is not None:
            self._filter_append(kwargs, f"Code eq '{code}'")
        return super().filter(**kwargs)
```

**Resulting OData query:**
```
GET /api/v1/{division}/logistics/Items?$filter=Code eq 'MYITEMCODE'&$select=ID,Code,CostPriceStandard,Description
```

**IMPORTANT**: Item Code does NOT have the 18-char padding! Only Account Code does.

### B3. Code → GUID Caching

**No SDK implements local caching of Code → GUID mappings.** Each lookup goes to the API every time.

The Python SDK has per-request caching for invoice lookups (via `_cached_remote` attribute in `ExactInvoice`), but no general Code → GUID cache.

**RECOMMENDATION for SUPWISE**: Build a local cache. With ~200 accounts and ~200,000 items, you want:
- Pre-warm cache: `GET /api/v1/{div}/crm/Accounts?$select=ID,Code` (one request, ~200 results)
- Pre-warm cache: `GET /api/v1/{div}/logistics/Items?$select=ID,Code` (paginated, ~200K results)
- Use `$top=60` default page size, follow `__next` links
- Refresh cache periodically (e.g., daily)

### B4. CRITICAL: ItemCode is READ-ONLY in Order Lines

**Confirmed from ExactOnlineRestApiReference metadata** (`meta-data.json`):

#### SalesOrderLines
| Field | Type | POST | PUT | Mandatory |
|-------|------|------|-----|-----------|
| **Item** | Edm.Guid | ✅ Yes | ✅ Yes | **YES** |
| **ItemCode** | Edm.String | ❌ No | ❌ No | No |

#### PurchaseOrderLines  
| Field | Type | POST | PUT | Mandatory |
|-------|------|------|-----|-----------|
| **Item** | Edm.Guid | ✅ Yes | ✅ Yes | **YES** |
| **ItemCode** | Edm.String | ❌ No | ❌ No | No |

**`ItemCode` has `"post": false, "put": false`** — it is a **read-only computed field** returned in GET responses but **cannot be sent in POST/PUT requests**.

**You MUST resolve Item Code → Item GUID before creating order lines.**

Similarly:
- `OrderedByName` → READ-ONLY. Use `OrderedBy` (GUID) instead.
- `SupplierCode` → READ-ONLY. Use `Supplier` (GUID) instead.
- `SupplierName` → READ-ONLY.

The Go SDK confirms this: `Item` is `*types.GUID` in the struct definition.

---

## C. Order Creation

### C1. SalesOrder POST Body

**Mandatory fields** (confirmed from metadata + n8n field config):
- `OrderedBy` — Customer Account GUID (Edm.Guid) — **POST only, cannot PUT**
- `SalesOrderLines` — Array of line objects

**Minimal SalesOrder POST:**
```json
POST /api/v1/{division}/salesorder/SalesOrders
Content-Type: application/json
Authorization: Bearer {access_token}

{
  "OrderedBy": "4f4f8200-77d5-4a70-b743-7f5c68b0a6d7",
  "SalesOrderLines": [
    {
      "Item": "a3b2c1d0-e4f5-6789-abcd-ef0123456789",
      "Quantity": 10.0,
      "UnitPrice": 25.50
    }
  ]
}
```

**Full SalesOrder POST (with common optional fields):**
```json
{
  "OrderedBy": "4f4f8200-77d5-4a70-b743-7f5c68b0a6d7",
  "OrderDate": "2026-04-07T00:00:00Z",
  "DeliveryDate": "2026-04-14T00:00:00Z",
  "Description": "Order for Customer ABC",
  "YourRef": "PO-12345",
  "Remarks": "Urgent delivery requested",
  "Currency": "EUR",
  "WarehouseID": "d1e2f3a4-b5c6-7890-abcd-ef1234567890",
  "SalesOrderLines": [
    {
      "Item": "a3b2c1d0-e4f5-6789-abcd-ef0123456789",
      "Description": "Widget Type A",
      "Quantity": 10.0,
      "UnitPrice": 25.50,
      "VATCode": "2  ",
      "Discount": 0.05
    },
    {
      "Item": "b4c3d2e1-f6a7-8901-bcde-f12345678901",
      "Description": "Widget Type B",
      "Quantity": 5.0,
      "NetPrice": 100.00
    }
  ]
}
```

**POST Response header**: Use `Prefer: return=representation` to get the created object back.
(Confirmed in n8n's `ExactOnline.node.js`: `{ headers: { Prefer: 'return=representation' } }`)

### C2. PurchaseOrder POST Body

**Mandatory fields** (confirmed from metadata + n8n field config):
- `Supplier` — Supplier Account GUID (Edm.Guid) — **POST only, cannot PUT**
- `PurchaseOrderLines` — Array of line objects

**Mandatory PurchaseOrderLine fields:**
- `Item` — Item GUID (Edm.Guid)
- `PurchaseOrderID` — Parent order GUID (when creating line separately; auto-set when nested)
- `QuantityInPurchaseUnits` — Quantity (Edm.Double) — **NOT `Quantity`** which is read-only!

**CRITICAL DIFFERENCE from SalesOrderLines**: PurchaseOrderLines use `QuantityInPurchaseUnits` (POST-able), NOT `Quantity` (read-only for PO lines).

**Minimal PurchaseOrder POST:**
```json
POST /api/v1/{division}/purchaseorder/PurchaseOrders
Content-Type: application/json
Authorization: Bearer {access_token}

{
  "Supplier": "5a6b7c8d-9e0f-1234-5678-9abcdef01234",
  "PurchaseOrderLines": [
    {
      "Item": "a3b2c1d0-e4f5-6789-abcd-ef0123456789",
      "QuantityInPurchaseUnits": 100.0,
      "UnitPrice": 12.75
    }
  ]
}
```

**Full PurchaseOrder POST:**
```json
{
  "Supplier": "5a6b7c8d-9e0f-1234-5678-9abcdef01234",
  "OrderDate": "2026-04-07T00:00:00Z",
  "ReceiptDate": "2026-04-21T00:00:00Z",
  "Description": "Purchase from Supplier XYZ",
  "YourRef": "SO-67890",
  "Remarks": "Standard monthly order",
  "Warehouse": "d1e2f3a4-b5c6-7890-abcd-ef1234567890",
  "PurchaseOrderLines": [
    {
      "Item": "a3b2c1d0-e4f5-6789-abcd-ef0123456789",
      "Description": "Raw Material A",
      "QuantityInPurchaseUnits": 100.0,
      "UnitPrice": 12.75
    }
  ]
}
```

### C3. SalesOrderLine Writable Fields

| Field | Type | POST | PUT | Notes |
|-------|------|------|-----|-------|
| Item | Edm.Guid | ✅ | ✅ | **Mandatory** — Item GUID |
| OrderID | Edm.Guid | ✅ | ✅ | **Mandatory** when creating standalone line |
| Quantity | Edm.Double | ✅ | ✅ | Number of items |
| UnitPrice | Edm.Double | ✅ | ✅ | Price per unit |
| NetPrice | Edm.Double | ✅ | ✅ | Net price (alternative to UnitPrice) |
| Description | Edm.String | ✅ | ✅ | Line description |
| Discount | Edm.Double | ✅ | ✅ | Discount fraction |
| VATCode | Edm.String | ✅ | ✅ | VAT code |
| DeliveryDate | Edm.DateTime | ✅ | ✅ | Line-level delivery date |
| Notes | Edm.String | ✅ | ✅ | Notes |
| ItemCode | Edm.String | ❌ | ❌ | **READ-ONLY** |
| ItemDescription | Edm.String | ❌ | ❌ | **READ-ONLY** |

### C4. PurchaseOrderLine Writable Fields

| Field | Type | POST | PUT | Notes |
|-------|------|------|-----|-------|
| Item | Edm.Guid | ✅ | ✅ | **Mandatory** — Item GUID |
| PurchaseOrderID | Edm.Guid | ✅ | ❌ | **Mandatory** — Parent order |
| QuantityInPurchaseUnits | Edm.Double | ✅ | ✅ | **Mandatory** — USE THIS, not Quantity |
| UnitPrice | Edm.Double | ✅ | ✅ | Price per purchase unit |
| NetPrice | Edm.Double | ✅ | ✅ | Net price |
| Description | Edm.String | ✅ | ✅ | Line description |
| Discount | Edm.Double | ✅ | ✅ | Discount |
| Quantity | Edm.Double | ❌ | ❌ | **READ-ONLY** — Use QuantityInPurchaseUnits |
| ItemCode | Edm.String | ❌ | ❌ | **READ-ONLY** |

---

## D. Pagination

### D1. All SDKs follow `__next` links

**Go SDK** (`api/response.go`):
```go
type listData struct {
    Results json.RawMessage `json:"results"`
    Next    string          `json:"__next"`
}
```

Pagination loop (`api/client.go`, `ListRequestAndDoAll`):
```go
var next = f.NextPage
for next != nil {
    _, l, rErr := c.NewRequestAndDo(ctx, "GET", next.String(), nil, &i)
    s = append(s, i...)
    next = l.NextPage
}
```

**Python SDK** (`exactonline/api/unwrap.py`):
```python
# GET responses: d.results contains data, d.__next contains next page URL
result_data, resource = self._rest_to_result_data_and_next(result_data)
ret.extend(result_data)
if resource:
    request = request.update(resource=resource)  # follow __next
```

**n8n integration** (`GenericFunctions.js`):
```javascript
do {
    responseData = await exactOnlineApiRequest.call(this, 'GET', uri, ...);
    returnData = returnData.concat(responseData.body.d.results);
    nextPageUrl = responseData.body.d.__next;
} while (returnData.length < limit && responseData.body.d.__next);
```

**`alexander-schillemans` Python** (`endpoints/base.py`):
```python
while '__next' in respJson['d']:
    nextUrl = respJson['d']['__next']
    # Strip base URL to get relative path
    nextUrl = nextUrl.replace(
        f'https://start.exactonline.be/api/v1/{self.api.division}/', '')
    status, headers, respJson = self.api.get(nextUrl)
```

### D2. Page Size

- Default page size returned by API appears to be **60 records** per page (based on n8n default limit)
- Python SDK configures `iteration_limit = 50` (max 50 pages before error) — this acts as a safety limit
- No SDK explicitly sets `$top` for pagination; they rely on the API default and follow `__next`

### D3. Bulk Endpoints

The n8n field config confirms bulk endpoints exist:
- `GET /api/v1/{division}/bulk/SalesOrder/SalesOrders` — GET only
- `GET /api/v1/{division}/bulk/SalesOrder/SalesOrderLines` — GET only
- `GET /api/v1/{division}/bulk/SalesOrder/GoodsDeliveries` — GET only

**Bulk endpoints are READ-ONLY (GET only).** They are used for mass data extraction, not creation. No SDK uses them for write operations.

### D4. Sync Endpoints

Also confirmed in n8n:
- `GET /api/v1/{division}/sync/SalesOrder/SalesOrders` — GET only
- `GET /api/v1/{division}/sync/PurchaseOrder/PurchaseOrders` — GET only

These are for incremental sync (delta queries) — also read-only.

### D5. No `$batch` endpoint found

No SDK implements or references a `$batch` endpoint.

---

## E. Rate Limiting

### E1. Rate Limit Headers

**Python SDK** (`exactonline/http.py`, `_update_ratelimiter_with_exactonline_headers`):
```python
# Daily limits
'X-RateLimit-Reset': 1638489600000       # Unix ms, when daily limit resets
'X-RateLimit-Limit': 9000                 # Max daily requests
'X-RateLimit-Remaining': 8924             # Remaining daily requests

# Minutely limits  
'X-RateLimit-Minutely-Reset': 1638447360000  # Unix ms, when minute limit resets
'X-RateLimit-Minutely-Limit': 100            # Max requests per minute
'X-RateLimit-Minutely-Remaining': 99         # Remaining requests this minute
```

### E2. Rate Limit Handling Strategies

| SDK | Strategy | Source |
|-----|----------|--------|
| `ossobv/exactonline` Python | **Best**: Proactive backoff. Tracks both minutely and daily limits. Waits before sending if `remaining < 1`. On 429, backs off and retries once. | `rawapi.py` + `http.py` |
| `@quantix-ict/exact-online` | Throws error on 429: `'Request Too Many Requests, try again later'` — no retry | `dist/cjs/index.js` |
| `alexander-schillemans` Python | Stalls 60s on minutely limit exhaustion. Throws on daily limit. | `api.py` |
| n8n `@datafix` version | **Improved**: `await sleep(waitTime)` when `x-ratelimit-minutely-remaining === "0"` | `GenericFunctions.js` |
| n8n original version | **Broken**: `setTimeout(() => {}, ...)` — non-blocking, doesn't actually wait! | `GenericFunctions.js` |

### E3. Python SDK Rate Limiter Detail

```python
class RateLimiter(object):
    def backoff(self):
        seconds = self._should_wait()
        if seconds > 0:
            self.wait(seconds)  # Actually blocks with time.sleep()
            return True
        return False

    def update(self, until, limit, remaining):
        # until is in milliseconds, convert to seconds
        until //= 1000
        self._reset_times[until] = (limit, remaining)

    def _should_wait(self):
        # Check all tracked windows; if any has remaining < 1, wait
        now = int(time() - 0.5)  # 0.5s offset for clock drift
        for key in self._reset_times:
            if now < key and self._reset_times[key][1] < 1:
                return max((key - now), 0)
        return 0
```

---

## F. Key Differences and Gotchas

### F1. BREAKING Issues (will cause failures)

1. **ItemCode is READ-ONLY** — Cannot send `ItemCode` in POST body for SalesOrderLines or PurchaseOrderLines. Must resolve to `Item` GUID first. (Severity: BREAKING)

2. **Account Code padding** — Account `Code` is 18 chars with leading spaces. OData filter must include padding: `Code eq '           1234567'`. (Severity: BREAKING if not padded)

3. **Token endpoint content-type** — Must be `application/x-www-form-urlencoded`, not `application/json`. (Severity: BREAKING)

4. **PurchaseOrderLine uses `QuantityInPurchaseUnits`** — NOT `Quantity` (which is read-only for PO lines). (Severity: BREAKING)

5. **Refresh token is single-use** — Must save new refresh_token after every refresh. Concurrent refreshes will fail. (Severity: BREAKING)

6. **`OrderedBy` is POST-only** — Cannot change customer on existing sales order via PUT. Same for `Supplier` on purchase orders. (Severity: BREAKING for updates)

### F2. SUBOPTIMAL Issues

7. **No bulk write API** — Bulk endpoints are GET-only. Orders must be created one at a time via POST. (Severity: SUBOPTIMAL — impacts throughput)

8. **No Item Code-based order creation** — All code→GUID resolution must happen client-side. No "create order by item code" shortcut. (Severity: SUBOPTIMAL)

### F3. COSMETIC Issues

9. **Item Code has no padding** — Unlike Account Code, Item Code is used as-is in filters (no 18-char padding). (Severity: COSMETIC — different from accounts)

---

## G. Key Code References

| Need | Best Source | File/Location |
|------|-----------|---------------|
| OAuth2 flow (Node.js) | `@quantix-ict/exact-online` | `dist/cjs/index.js` — cleanest modern Node.js implementation |
| OAuth2 flow (Python) | `ossobv/exactonline` | `rawapi.py` + `api/autorefresh.py` — most robust |
| Order struct definitions | `mcnijman/go-exactonline` | `services/salesorder/sales_orders.go`, `services/purchaseorder/purchase_orders.go` |
| API field metadata | `DannyvdSluijs/ExactOnlineRestApiReference` | `meta-data.json` — authoritative for POST/PUT/GET per field |
| Rate limiting | `ossobv/exactonline` | `rawapi.py` + `http.py` — best implementation |
| Pagination | All SDKs | Follow `d.__next` URL from JSON response |
| Account Code padding | `ossobv/exactonline` | `api/relations.py` — `'%18s'` format string |
| n8n field config (mandatory fields) | `n8n-nodes-exact-online` | `fieldConfigArray.json` |
| OData filter syntax | `alexander-schillemans/python-exact-online` | `endpoints/base.py` |

---

## H. Recommendations for SUPWISE NestJS Backend

### H1. Architecture Recommendations

1. **Build a GUID cache**: Pre-fetch all Account and Item records at startup, cache Code→GUID maps in Redis or memory. Refresh daily.

2. **Token management**: Store access_token, refresh_token, and access_expiry in database. Use a mutex/lock to prevent concurrent refreshes. Refresh proactively at ~9.5 minutes (30s before 10-min expiry).

3. **One-time OAuth setup**: Build a simple `/exact/auth/init` and `/exact/auth/callback` endpoint pair for the initial browser-based OAuth flow. Once tokens are obtained, the service runs unattended.

4. **Rate limiter**: Track both minutely (100/min) and daily (9000/day) limits from response headers. Back off proactively when approaching limits.

### H2. Item Resolution Strategy (200,000 items)

With 200K items, pre-fetching all at once is expensive but feasible:
```
GET /api/v1/{div}/logistics/Items?$select=ID,Code
```
At 60 items/page × ~3,334 pages. At 100 requests/minute limit, this takes ~33 minutes for initial load.

**Alternative**: Use bulk endpoint for faster reads:
```
GET /api/v1/{div}/bulk/Logistics/Items?$select=ID,Code
```
Bulk endpoints return more records per page (1000+), reducing pages dramatically.

### H3. Account Code Padding

```typescript
function padAccountCode(code: string): string {
    return code.padStart(18, ' ');
}

// OData filter:
const filter = `Code eq '${padAccountCode(accountCode)}'`;
```

### H4. Order Creation Flow

```
1. Resolve OrderedBy/Supplier code → Account GUID (from cache)
2. For each line: Resolve Item code → Item GUID (from cache)  
3. POST /api/v1/{division}/salesorder/SalesOrders with GUIDs
4. Parse response for OrderID
```
