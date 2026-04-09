# Exact Online API — Documentation Research Findings

**Source**: Official Exact Online developer documentation + verified community sources
**Date**: 2026-04-07
**Researcher**: Doc Researcher agent

---

## Topic 1: Authorization / OAuth2

### 1.1 API Keys / Service Accounts

**Finding**: ❓ No evidence of API keys or service accounts for server-to-server authentication.

Exact Online uses **exclusively OAuth 2.0 Authorization Code flow** for API access. There is no `client_credentials` grant type, no API keys, and no service accounts.

- The OAuth2 auth endpoint at `https://start.exactonline.nl/api/oauth2/auth` serves a browser login page (confirmed by HTTP 200 + HTML response).
- The token endpoint at `https://start.exactonline.nl/api/oauth2/token` exists (returns HTTP 400 when called without proper POST body, as expected).
- ✅ Confirmed from Picqer PHP SDK source: only `authorization_code` and `refresh_token` grant types are used.
  - Source: https://github.com/picqer/exact-php-client/blob/master/src/Picqer/Financials/Exact/Connection.php

**Implication for SUPWISE**: An initial interactive browser login is **always required** at least once to obtain the first authorization code. After that, the refresh token mechanism can maintain access.

### 1.2 App Registration

**Finding**: ⚠️ Likely — based on SDK documentation and community sources.

- You register an app at the **Exact App Center** to obtain `Client ID` and `Client Secret`.
- You must also set a `Callback URL` (redirect URI) for the OAuth dance.
- ⚠️ Private/internal apps (not published in App Center) appear to be possible — integration partners commonly register apps that only their own account uses.
- Source: Picqer SDK README — "Set up an App at the Exact App Center to retrieve your Client ID and Client Secret."
  - URL: https://github.com/picqer/exact-php-client/blob/master/README.md

### 1.3 OAuth2 Flow

**Confidence**: ✅ HIGH — confirmed from SDK source code and endpoint testing.

#### Authorization URL (NL region)
```
https://start.exactonline.nl/api/oauth2/auth
```

**Required parameters** (GET request):
| Parameter | Value |
|---|---|
| `client_id` | Your app's client ID |
| `redirect_uri` | Your registered callback URL |
| `response_type` | `code` |
| `state` | Optional CSRF token |
| `force_login` | Optional, `0` or `1` |

**Example**:
```
https://start.exactonline.nl/api/oauth2/auth?client_id=YOUR_CLIENT_ID&redirect_uri=https://yourapp.com/callback&response_type=code
```

#### Token URL (NL region)
```
https://start.exactonline.nl/api/oauth2/token
```

**Initial token exchange** (POST, form-encoded):
```
POST /api/oauth2/token
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code
&client_id=YOUR_CLIENT_ID
&client_secret=YOUR_CLIENT_SECRET
&redirect_uri=https://yourapp.com/callback
&code=AUTHORIZATION_CODE_FROM_CALLBACK
```

**Token refresh** (POST, form-encoded):
```
POST /api/oauth2/token
Content-Type: application/x-www-form-urlencoded

grant_type=refresh_token
&client_id=YOUR_CLIENT_ID
&client_secret=YOUR_CLIENT_SECRET
&refresh_token=STORED_REFRESH_TOKEN
```

**Response** (JSON):
```json
{
  "access_token": "...",
  "refresh_token": "...",
  "expires_in": "600"
}
```

#### Supported grant types
- ✅ `authorization_code` — confirmed
- ✅ `refresh_token` — confirmed
- ❌ `client_credentials` — **NOT supported**
- Source: https://github.com/picqer/exact-php-client/blob/master/src/Picqer/Financials/Exact/Connection.php (lines showing both grant types in acquireAccessToken method)

### 1.4 Token Lifetimes

**Confidence**: ⚠️ MEDIUM — `expires_in` value visible in SDK, lifetimes based on well-known community knowledge.

| Token | Lifetime | Source |
|---|---|---|
| Access token | **~10 minutes** (600 seconds) | ⚠️ Well-known community knowledge; `expires_in` field parsed by SDK |
| Refresh token | ⚠️ **~30 days** (unconfirmed exact value) | ⚠️ Community sources; not found in official docs |

#### Refresh token rotation
- ✅ **YES, the refresh token rotates**: Each token refresh returns a **new** `refresh_token` in the response.
- The SDK stores both the new `access_token` AND new `refresh_token` from each refresh call.
- Source: Picqer SDK `acquireAccessToken()` method:
  ```php
  $this->accessToken = $body['access_token'];
  $this->refreshToken = $body['refresh_token'];  // NEW refresh token!
  $this->tokenExpires = $this->getTimestampFromExpiresIn($body['expires_in']);
  ```
- **CRITICAL**: The old refresh token becomes invalid after use. You MUST persist the new refresh token.

#### What happens if the refresh token expires?
- ⚠️ If the refresh token expires (reportedly ~30 days of non-use), you need a **new interactive browser login** to get a fresh authorization code.
- The SDK handles this by redirecting for re-authorization when `needsAuthentication()` is true (no valid tokens).

### 1.5 Unattended / Daemon Access

**Finding**: ⚠️ MEDIUM confidence — pieced together from SDK patterns and community knowledge.

**There is no official "daemon mode" or server-to-server flow.** However, unattended access IS achievable:

1. **Initial setup**: One-time interactive browser login to obtain authorization code
2. **Token persistence**: Store the refresh token securely in a database
3. **Automatic refresh**: Before each API call, check if access token is expired; if so, use refresh token to get new pair
4. **Keep-alive**: As long as you make at least one API call within the refresh token expiry window (~30 days), the refresh token renews itself
5. **Lock mechanism**: The Picqer SDK provides `acquireAccessTokenLockCallback` / `acquireAccessTokenUnlockCallback` to prevent race conditions when multiple processes try to refresh simultaneously

**For SUPWISE**: If the system makes API calls at least once every 30 days (which it will, given daily order pushes), the tokens will stay alive indefinitely after the initial browser login.

### 1.6 How Integration Partners Maintain Persistent Access

**Finding**: ⚠️ MEDIUM — inferred from SDK patterns.

Integration partners like Debesis, Emagine, etc. maintain persistent access by:
1. Storing refresh tokens in a persistent database
2. Implementing automatic token refresh on every API call
3. Having a monitoring/alerting system if tokens expire
4. Having a re-authorization flow available as a fallback
5. Using lock mechanisms to prevent concurrent refresh token use

---

## Topic 2: GUID Resolution & OData Queries

### 2.1 Account Lookup by Code

**Confidence**: ✅ HIGH — confirmed from official API docs AND SDK source code.

**YES**, the Accounts endpoint supports OData `$filter` on the `Code` field.

#### ⚠️ CRITICAL CAVEAT: Account Code has leading spaces!

From the official API documentation:
> **Code**: Unique key, fixed length numeric string with leading spaces, length 18. **IMPORTANT: When you use OData $filter on this field you have to make sure the filter parameter contains the leading spaces**

- Source: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=CRMAccounts

The Picqer SDK handles this with `sprintf('%18s', $code)` — padding the code to 18 characters with leading spaces.

#### Example query:
```http
GET /api/v1/{division}/crm/Accounts?$filter=Code eq '              1234'&$top=1&$select=ID,Code,Name
```

Note: The code `1234` must be padded to 18 chars: `'              1234'` (14 spaces + 4 digit code)

#### Response format (OData v2):
```json
{
  "d": {
    "results": [
      {
        "ID": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
        "Code": "              1234",
        "Name": "Customer Name"
      }
    ]
  }
}
```

#### Key fields:
| Field | Type | Filter | Description |
|---|---|---|---|
| `ID` | Edm.Guid | ✅ YES | Primary key (the GUID you need) |
| `Code` | Edm.String | ✅ YES | Unique key, 18-char fixed-length with leading spaces |
| `Name` | Edm.String | ✅ YES | Account name (mandatory on create) |
| `SearchCode` | Edm.String | ✅ YES | Search code |
| `Status` | Edm.String | ✅ YES | Customer status |

### 2.2 Item Lookup by Code

**Confidence**: ✅ HIGH — confirmed from official API docs.

**YES**, the Items endpoint supports OData `$filter` on the `Code` field.

#### Example query:
```http
GET /api/v1/{division}/logistics/Items?$filter=Code eq 'ITEMCODE123'&$top=1&$select=ID,Code,Description
```

**Item Code is mandatory and unique on create** (Mandatory=True in API docs), but the uniqueness constraint is NOT explicitly documented for existing data queries.

#### Key fields:
| Field | Type | Filter | Mandatory | Description |
|---|---|---|---|---|
| `ID` | Edm.Guid | ✅ YES | No | Primary key (the GUID you need) |
| `Code` | Edm.String | ✅ YES | **Yes** | Item code |
| `Description` | Edm.String | ✅ YES | **Yes** | Description |
| `Barcode` | Edm.String | ✅ YES | No | Barcode (numeric string) |
| `IsSalesItem` | Edm.Byte | ✅ YES | No | Is sales item |
| `IsPurchaseItem` | Edm.Byte | ✅ YES | No | Is purchase item |

- Source: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=LogisticsItems

#### What if Code is not unique?
- Item `Code` is mandatory and the API docs describe it as "Item code" without stating uniqueness.
- ⚠️ However, in practice, Exact Online enforces Item Code uniqueness per division.
- If using `$filter=Code eq 'X'`, you should use `$top=1` and expect either 0 or 1 results.

### 2.3 Pagination

**Confidence**: ✅ HIGH — confirmed from official API resources page.

#### From the official API reference page:
> "Most of the REST API have a page size of 60. The bulk and sync endpoints have a pagesize of 1000. It is recommended to use the sync endpoints where possible."

- Source: https://start.exactonline.nl/docs/HlpRestAPIResources.aspx

| Parameter | Value |
|---|---|
| Default page size (standard endpoints) | **60** |
| Default page size (bulk/sync endpoints) | **1000** |
| `$top` parameter | ✅ Supported (some endpoints require it) |
| Maximum `$top` | ❓ Not documented; SDK uses `$top=1` for lookups |
| `$skiptoken` | ⚠️ Used via `__next` URL in OData response |

#### How pagination works:
The Exact Online API uses OData v2 pagination with the `__next` link:

```json
{
  "d": {
    "results": [ ... ],
    "__next": "https://start.exactonline.nl/api/v1/{division}/crm/Accounts?$skiptoken=guid'xxx'"
  }
}
```

- If `__next` is present, there are more pages.
- Follow the `__next` URL to get the next page.
- The SDK tracks this via `$this->nextUrl = $json['d']['__next']`
- Source: Picqer SDK Connection.php parseResponse method

### 2.4 Order Line Fields — CRITICAL

**Confidence**: ✅ HIGH — confirmed from official API documentation.

#### SalesOrderLines — Mandatory fields for POST:

When creating a SalesOrder (POST to `/api/v1/{division}/salesorder/SalesOrders`):

**SalesOrder (header) mandatory fields**:
| Field | Type | Description |
|---|---|---|
| `OrderedBy` | Edm.Guid | ✅ **MANDATORY** — Customer GUID (Account ID) |
| `SalesOrderLines` | Collection | ✅ **MANDATORY** — Array of order lines |

**SalesOrderLine mandatory fields (nested in SalesOrderLines)**:
| Field | Type | Description |
|---|---|---|
| `Item` | **Edm.Guid** | ✅ **MANDATORY** — Item GUID |
| `OrderID` | Edm.Guid | ✅ **MANDATORY** (auto-filled when nested in SalesOrder POST) |

**⚠️ CRITICAL: `Item` (GUID) is MANDATORY. `ItemCode` (string) is NOT accepted as a substitute.**

- `ItemCode` is listed as `Mandatory=False`, type `Edm.String`, description "Code of Item" — it is a **read-only** reference field.
- You MUST resolve the Item Code → GUID **before** creating order lines.
- Source: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrderLines

**Additional commonly-used SalesOrderLine fields** (not mandatory but typically provided):
- `Quantity` (Edm.Double) — number of items
- `UnitPrice` (Edm.Double) — price per unit
- `Description` (Edm.String) — line description
- `VATCode` (Edm.String) — VAT code
- `DeliveryDate` (Edm.DateTime) — delivery date

**Example SalesOrder POST body**:
```json
{
  "OrderedBy": "customer-guid-here",
  "OrderDate": "2026-04-07",
  "SalesOrderLines": [
    {
      "Item": "item-guid-here",
      "Quantity": 10,
      "UnitPrice": 25.50
    }
  ]
}
```

From the "Good to know" section:
> "You must include a parameter for 'SalesOrderLines' to add sales order lines when you POST to the SalesOrders endpoint."

---

#### PurchaseOrderLines — Mandatory fields for POST:

When creating a PurchaseOrder (POST to `/api/v1/{division}/purchaseorder/PurchaseOrders`):

**PurchaseOrder (header) mandatory fields**:
| Field | Type | Description |
|---|---|---|
| `Supplier` | Edm.Guid | ✅ **MANDATORY** — Supplier GUID (Account ID) |
| `PurchaseOrderLines` | Collection | ✅ **MANDATORY** — Array of order lines |

**PurchaseOrderLine mandatory fields (nested in PurchaseOrderLines)**:
| Field | Type | Description |
|---|---|---|
| `Item` | **Edm.Guid** | ✅ **MANDATORY** — Item GUID |
| `PurchaseOrderID` | Edm.Guid | ✅ **MANDATORY** (auto-filled when nested) |
| `QuantityInPurchaseUnits` | **Edm.Double** | ✅ **MANDATORY** — "Use this field when creating a purchase order" |

**⚠️ CRITICAL: Same as SalesOrderLines — `Item` (GUID) is MANDATORY.**
- `ItemCode` is NOT a substitute.
- `QuantityInPurchaseUnits` is mandatory for PurchaseOrderLines (unlike SalesOrderLines where `Quantity` is optional).

From the "Good to know" section:
> "When using the POST method of this endpoint to create a purchase order, it is mandatory to provide a valid supplier and purchase order lines."
> "When using the POST method of this endpoint to create a purchase order line it is mandatory to provide an Item and Quantity."

- Source: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrderLines

**Example PurchaseOrder POST body**:
```json
{
  "Supplier": "supplier-guid-here",
  "PurchaseOrderLines": [
    {
      "Item": "item-guid-here",
      "QuantityInPurchaseUnits": 100,
      "UnitPrice": 15.00
    }
  ]
}
```

### 2.5 Batch/Bulk Operations

**Confidence**: ✅ HIGH (bulk/sync endpoints) / ⚠️ MEDIUM (OData $batch)

#### Bulk Endpoints (GET only — for reading large datasets)
✅ Confirmed from official API resources page:

| Endpoint | URI | Page Size |
|---|---|---|
| Bulk Accounts | `/api/v1/{division}/bulk/CRM/Accounts` | 1000 |
| Bulk Items | `/api/v1/{division}/bulk/Logistics/Items` | 1000 |
| Bulk SalesOrders | `/api/v1/{division}/bulk/SalesOrder/SalesOrders` | 1000 |
| Bulk SalesOrderLines | `/api/v1/{division}/bulk/SalesOrder/SalesOrderLines` | 1000 |

- Source: https://start.exactonline.nl/docs/HlpRestAPIResources.aspx

#### Sync Endpoints (GET only — for incremental sync)
✅ Confirmed:

| Endpoint | URI |
|---|---|
| Sync Accounts | `/api/v1/{division}/sync/CRM/Accounts` |
| Sync Items | `/api/v1/{division}/sync/Logistics/Items` |
| Sync PurchaseOrders | `/api/v1/{division}/sync/PurchaseOrder/PurchaseOrders` |

The official docs recommend: "It is recommended to use the sync endpoints where possible."

#### OData `$filter` with `or` operator
⚠️ OData `$filter` supports `or` operator in theory (OData v2 spec), but:
- The Exact API documentation says "This API allows to filter only on specific fields" — it enforces per-field filter support.
- ❓ Whether `$filter=Code eq 'A' or Code eq 'B'` works is not explicitly documented. It may work for fields that support filtering.

#### OData `$batch` endpoint
❓ **Unknown** — No evidence of `$batch` support in Exact Online API documentation.

#### Bulk POST/Create
❓ **Not supported** — Bulk and Sync endpoints appear to be **GET-only** for reading data. Order creation must be done one-at-a-time via the standard endpoints.

### 2.6 Rate Limits

**Confidence**: ✅ HIGH — confirmed from SDK source code showing exact header names.

#### Rate Limit Headers
✅ Confirmed from Picqer SDK `extractRateLimits()`:

| Header | Description |
|---|---|
| `X-RateLimit-Limit` | Daily limit total |
| `X-RateLimit-Remaining` | Daily limit remaining |
| `X-RateLimit-Reset` | Daily limit reset timestamp |
| `X-RateLimit-Minutely-Limit` | Per-minute limit total |
| `X-RateLimit-Minutely-Remaining` | Per-minute limit remaining |
| `X-RateLimit-Minutely-Reset` | Per-minute limit reset timestamp (milliseconds) |

- Source: https://github.com/picqer/exact-php-client/blob/master/src/Picqer/Financials/Exact/Connection.php

#### Rate Limit Values
⚠️ The exact current values are not confirmed from official documentation but are widely reported in community sources:

| Limit | Value | Scope |
|---|---|---|
| Per-minute limit | ⚠️ ~60 requests/minute | Per app per company |
| Daily limit | ⚠️ ~5,000 requests/day | Per app per company |

#### Rate Limit Behavior
- ⚠️ HTTP status **429 Too Many Requests** is expected when rate limited (standard HTTP pattern).
- The SDK implements `waitIfMinutelyRateLimitHit()` which sleeps until the `X-RateLimit-Minutely-Reset` timestamp.
- The reset timestamp is in **milliseconds** (SDK converts: `$minutelyReset / 1000`).
- Source: Picqer SDK Connection.php

#### Impact on SUPWISE
With ~200,000 items to potentially resolve and 60 req/min + 5000 req/day limits:
- **Pre-caching all items**: 200,000 items ÷ 1000 per page (bulk endpoint) = 200 requests = feasible in one day
- **Per-order lookups**: Must be carefully rate-limited
- **Recommendation**: Use bulk/sync endpoints to build and maintain a local GUID cache

---

## Topic 3: Test Environment / Sandbox

### 3.1 Sandbox Availability

**Finding**: ❓ Unknown — could not find official documentation about a dedicated sandbox.

- No separate test URL was found (no `test.exactonline.nl` or similar).
- The API base URL for NL is `https://start.exactonline.nl` for all environments.
- ❓ Exact Online does not appear to offer a separate sandbox API environment.
- Source: Picqer SDK does not reference any sandbox URL; only production URLs per country.

### 3.2 Demo Company

**Finding**: ⚠️ MEDIUM — based on community knowledge.

- ⚠️ Exact Online typically provides a **demo company** (demonstratiebedrijf) when you create a new Exact Online account or trial.
- This demo company contains sample data and can be used for API testing.
- ❓ Whether it has rate limit exemptions is unknown.

### 3.3 Copy Division ("Kopie-divisie")

**Finding**: ⚠️ MEDIUM — community knowledge.

- ⚠️ Exact Online supports creating a copy of an existing division (administration) for testing purposes.
- This is done from the Exact Online UI, not via API.
- The copy division gets its own division code and data.
- ❓ Details on how to create one, and whether it has separate rate limits, are not confirmed from official docs.

### 3.4 Regional Base URLs

✅ Confirmed from SDK documentation:

| Region | Base URL |
|---|---|
| Netherlands | `https://start.exactonline.nl` |
| Germany | `https://start.exactonline.de` |
| Belgium | `https://start.exactonline.be` |
| UK | `https://start.exactonline.co.uk` |
| USA | `https://start.exactonline.com` |
| Spain | `https://start.exactonline.es` |
| France | `https://start.exactonline.fr` |

- Source: Picqer SDK README, referencing https://developers.exactonline.com/#Exact%20Online%20sites.html

### 3.5 Division Discovery

✅ Confirmed from official API reference:

```http
GET /api/v1/current/Me?$select=CurrentDivision
```

To get all accessible divisions:
```http
GET /api/v1/{division}/system/Divisions
```

To get all divisions for the current license:
```http
GET /api/v1/{division}/system/AllDivisions
```

- Source: https://start.exactonline.nl/docs/HlpRestAPIResources.aspx

---

## Summary of Critical Findings for SUPWISE

### GUID Resolution is MANDATORY
Both `SalesOrderLines.Item` and `PurchaseOrderLines.Item` require **GUID** (Edm.Guid). `ItemCode` is NOT a substitute. Similarly, `OrderedBy` (SalesOrder) and `Supplier` (PurchaseOrder) require Account GUIDs.

### Recommended Architecture
1. **Build a local cache** of Item GUIDs using bulk endpoint (`/api/v1/{division}/bulk/Logistics/Items`) — returns 1000 per page
2. **Build a local cache** of Account GUIDs using bulk endpoint (`/api/v1/{division}/bulk/CRM/Accounts`) — returns 1000 per page
3. **Maintain caches** using sync endpoints (`/api/v1/{division}/sync/Logistics/Items`, `/api/v1/{division}/sync/CRM/Accounts`)
4. **Resolve codes locally** from cache instead of making per-order API calls
5. **Rate limit all API calls** — max 60/min, ~5000/day

### Account Code Gotcha
Account `Code` is a **fixed-length 18-character string with leading spaces**. When filtering:
```
$filter=Code eq '              1234'
```
Use `sprintf('%18s', code)` or equivalent padding.

### Token Management
- Initial one-time browser login required
- Store refresh token persistently
- Refresh tokens rotate — always save the new one
- Keep-alive: any API call within ~30 days renews the refresh token

---

## Unanswered Questions

| # | Question | Status |
|---|---|---|
| 1 | Exact refresh token lifetime — is it exactly 30 days? | ❓ Not found in official docs |
| 2 | Can `$filter` use `or` operator (e.g., `Code eq 'A' or Code eq 'B'`)? | ❓ Not explicitly documented |
| 3 | Maximum `$top` value for standard endpoints | ❓ Not documented |
| 4 | Does OData `$batch` work? | ❓ No evidence found |
| 5 | Is there a dedicated sandbox/test environment? | ❓ Not found in accessible docs |
| 6 | Exact rate limit numbers (60/min, 5000/day) — are these still current? | ⚠️ Community knowledge only |
| 7 | Rate limit scope — per app, per company, or per division? | ⚠️ Likely per app per company |
| 8 | Can you register a private app without publishing to App Center? | ⚠️ Likely yes, not confirmed |
| 9 | HTTP status code for rate limiting — is it 429? | ⚠️ Likely, not explicitly confirmed |
| 10 | Does `ItemCode` work as an alternative to `Item` GUID in POST (undocumented)? | ❓ API docs say NO (Item is mandatory, ItemCode is read-only) |

---

## Sources Consulted

| Source | URL | Accessible? |
|---|---|---|
| Exact Online API Reference | https://start.exactonline.nl/docs/HlpRestAPIResources.aspx | ✅ Yes |
| SalesOrders endpoint docs | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrders | ✅ Yes |
| SalesOrderLines endpoint docs | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrderLines | ✅ Yes |
| PurchaseOrders endpoint docs | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrders | ✅ Yes |
| PurchaseOrderLines endpoint docs | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrderLines | ✅ Yes |
| CRM Accounts endpoint docs | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=CRMAccounts | ✅ Yes |
| Logistics Items endpoint docs | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=LogisticsItems | ✅ Yes |
| Exact Developer Portal | https://developer.exactonline.com/ | ❌ JS-rendered, content not extractable |
| Exact Support Knowledge Base | https://support.exactonline.com/community/s/knowledge-base | ❌ JS-rendered (Salesforce community) |
| Picqer PHP SDK (community) | https://github.com/picqer/exact-php-client | ✅ Yes — excellent reference implementation |
| Picqer SDK Connection.php | (see above) | ✅ OAuth URLs, rate limit headers, token handling |
| Picqer SDK Findable.php | (see above) | ✅ OData query patterns, Account Code padding |

**Note**: The Exact Developer Portal and Support Knowledge Base are JavaScript-rendered Salesforce community sites that cannot be scraped via curl. The official API reference at `start.exactonline.nl/docs/` returns server-rendered HTML and was successfully parsed for field-level documentation.
