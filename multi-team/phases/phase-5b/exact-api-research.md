# Exact Online REST API — Research Handoff for SUPWISE Meeting

**Researched:** 2026-04-07
**Sources:** Official Exact API docs, Picqer PHP SDK, ossobv Python SDK, Quantix Node.js SDK, Go SDK, n8n integrations, ExactOnlineRestApiReference metadata
**Confidence:** HIGH (cross-verified between official docs + 5 SDKs + machine-readable API metadata)
**Purpose:** Meeting preparation with Exact Online PM & API developer

---

## Executive Summary

- **Geen API keys of service accounts** — OAuth2 Authorization Code flow is de ENIGE optie. Eenmalige browser login vereist, daarna onbeperkt automatisch vernieuwen. Bevestigd in alle 5 SDKs en officiële docs.
- **`ItemCode` is READ-ONLY** — Je KUNT GEEN orders aanmaken met artikelcodes. Elke orderregel vereist een Item GUID. Dit betekent dat ~200K item codes eerst naar GUIDs moeten worden vertaald. Bevestigd in API metadata (`"post": false, "put": false`) en alle SDKs.
- **Account Code heeft 18-karakter padding met spaties** — Filter `Code eq '              1234'` (14 spaties + 4 cijfers). Zonder padding geen resultaten. Bevestigd in officiële API docs en Python/PHP SDKs.
- **Rate limits zijn ONDUIDELIJK** — Bronnen spreken elkaar tegen: 60/min of 100/min? 5.000/dag of 9.000/dag? Waarschijnlijk tier-afhankelijk. **Vraag aan Exact in meeting.**
- **Geen sandbox/testomgeving gevonden** — Geen apart test-URL gedocumenteerd. Opties: demo-bedrijf in proefaccount of kopie-divisie. **Vraag aan Exact in meeting.**

---

## 1. Autorisatie — OAuth2 (Geen API Keys)

### 1.1 Geen API Keys of Service Accounts

**Confidence: ✅ HIGH** — Bevestigd door officiële docs + alle 5 SDKs.

Exact Online gebruikt **uitsluitend OAuth 2.0 Authorization Code flow** voor API toegang:
- ❌ Geen `client_credentials` grant type
- ❌ Geen API keys
- ❌ Geen service accounts
- ✅ Alleen `authorization_code` + `refresh_token` grant types

Alle 5 onderzochte SDKs (Node.js, Python×2, PHP, Go) implementeren uitsluitend deze flow. Geen enkele SDK heeft een daemon-modus of server-to-server flow.

**Bronnen:**
- Picqer PHP SDK `Connection.php`: alleen `authorization_code` en `refresh_token` in `acquireAccessToken()`
- Quantix Node.js SDK: alleen `refresh_token` grant type in `refreshTokens()`
- ossobv Python SDK: alleen `authorization_code` en `refresh_token` in `rawapi.py`

### 1.2 App Registratie

Registreer een app bij het **Exact App Center** om `Client ID` en `Client Secret` te verkrijgen:
- Stel een `Callback URL` (redirect URI) in voor de OAuth dance
- **Private/interne apps** (niet gepubliceerd in App Center) zijn waarschijnlijk mogelijk — integratiepartners registreren vaak apps die alleen door hun eigen account worden gebruikt
- ⚠️ Of dit officieel ondersteund is, moet bevestigd worden in het meeting

**Bron:** Picqer SDK README — "Set up an App at the Exact App Center to retrieve your Client ID and Client Secret."

### 1.3 OAuth2 Flow — Stap voor Stap

#### Stap 1: Eenmalige Browser Login

**Authorization URL (NL regio):**
```
https://start.exactonline.nl/api/oauth2/auth
```

**Parameters (GET request):**
| Parameter | Waarde | Verplicht |
|-----------|--------|-----------|
| `client_id` | App's client ID | ✅ Ja |
| `redirect_uri` | Geregistreerde callback URL | ✅ Ja |
| `response_type` | `code` | ✅ Ja |
| `state` | CSRF token | Optioneel |
| `force_login` | `0` of `1` | Optioneel |

**Voorbeeld volledige URL:**
```
https://start.exactonline.nl/api/oauth2/auth?client_id=YOUR_CLIENT_ID&redirect_uri=https://yourapp.com/callback&response_type=code
```

De gebruiker logt in via de browser. Na succesvolle login redirect Exact naar de callback URL met een `code` parameter.

#### Stap 2: Token Exchange

**Token URL (NL regio):**
```
https://start.exactonline.nl/api/oauth2/token
```

**⚠️ KRITIEK: Content-Type MOET `application/x-www-form-urlencoded` zijn, NIET JSON!**
Bevestigd in alle 3 onderzochte talen (Node.js, Python, PHP).

**Request:**
```http
POST /api/oauth2/token HTTP/1.1
Host: start.exactonline.nl
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code&client_id=YOUR_CLIENT_ID&client_secret=YOUR_CLIENT_SECRET&redirect_uri=https://yourapp.com/callback&code=AUTHORIZATION_CODE_FROM_CALLBACK
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

#### Stap 3: Token Refresh (automatisch)

**Request:**
```http
POST /api/oauth2/token HTTP/1.1
Host: start.exactonline.nl
Content-Type: application/x-www-form-urlencoded

grant_type=refresh_token&client_id=YOUR_CLIENT_ID&client_secret=YOUR_CLIENT_SECRET&refresh_token=STORED_REFRESH_TOKEN
```

**Response:** Zelfde formaat als token exchange — bevat NIEUWE `access_token` EN NIEUWE `refresh_token`.

### 1.4 Token Lifetimes

| Token | Lifetime | Bron | Confidence |
|-------|----------|------|------------|
| Access token | **~10 min (600s)** | `expires_in` field in alle SDK responses | ✅ HIGH |
| Refresh token | **~30 dagen** | Community kennis; NIET officieel gedocumenteerd | ⚠️ MEDIUM |

### 1.5 Refresh Token Rotatie — KRITIEK

**Confidence: ✅ HIGH** — Bevestigd in alle 5 SDKs.

De refresh token is **SINGLE-USE**:
1. Elke token refresh retourneert een **NIEUWE** refresh token
2. De **OUDE** refresh token wordt **DIRECT ongeldig**
3. Je MOET de nieuwe refresh token **atomisch** opslaan
4. Als je de nieuwe refresh token niet opslaat, verlies je permanent toegang
5. **Concurrent refreshes zullen falen** — gebruik een mutex/lock

**Quantix SDK bevestigt dit met expliciete mutex:**
```javascript
// Prevents concurrent refresh requests
if (this.refreshing) {
    await sleep(1000);
    return;
}
this.refreshing = true;
```

**Opslagpatronen uit SDKs:**
| SDK | Opslag Methode |
|-----|----------------|
| Quantix Node.js | Bestandssysteem: `os.tmpdir()/quantix-ict-exact/refresh.json` |
| ossobv Python | Configureerbaar: INI bestand, database, etc. |
| alexander-schillemans Python | Bestandssysteem: `cache/{clientId}.txt` als JSON |
| exact-online npm | Alleen in geheugen (niet persistent!) |

**Voor SUPWISE's NestJS backend:** Gebruik een database tabel met atomic read-write voor token opslag.

### 1.6 Unattended/Daemon Access Strategie

**Er is geen officiele "daemon mode"**, maar onbeheerde toegang IS haalbaar:

1. **Eenmalig:** Admin voltooit browser OAuth flow → verkrijgt authorization code
2. **Eerste exchange:** Server wisselt code in voor access_token + refresh_token
3. **Automatische refresh loop:** Server ververst token ~30 seconden voor 10-min verloop
4. **Persist nieuwe tokens:** Na elke refresh wordt nieuwe refresh_token opgeslagen
5. **Keep-alive:** Zolang je minstens elke ~30 dagen een API call maakt, blijven tokens geldig

**Best practice (uit Python SDK docs):**
> - Refresh niet te laat — doe het voor verloop
> - Refresh niet te vroeg — wacht tot ~9 min 30 sec
> - Aanbevolen: refresh wanneer < 30 seconden resterend

**Alle SDKs implementeren automatische refresh op 401:**
| SDK | Patroon |
|-----|---------|
| exact-online npm | `checkAuth()` voor elk request — refresht als `expires < Date.now()` |
| Quantix Node.js | Detecteert `status === 401` → `refreshTokens()` → retry |
| ossobv Python | Proactieve refresh 30s voor verloop + fallback retry op 401 |
| alexander-schillemans Python | `isTokenDueRenewal()` voor elk request (30s buffer) |

### 1.7 Vragen voor het Meeting — Autorisatie

1. **Is er een service account / API key optie** voor server-to-server integratie zonder browser login?
2. **Wat is de exacte levensduur** van de refresh token? (Wij schatten ~30 dagen, maar dit is niet officieel gedocumenteerd)
3. **Kunnen we een private app registreren** die niet gepubliceerd hoeft te worden in het App Center?
4. **Wat gebeurt er als het refresh token verloopt** terwijl het systeem tijdelijk offline is? Is er een manier om opnieuw te autoriseren zonder eindgebruiker-interactie?
5. **Is er een webhook/push optie** voor wijzigingen, zodat we niet continu hoeven te pollen?

---

## 2. GUID Resolutie — Codes naar GUIDs

### 2.1 Account (Relatie) Opzoeken op Code

**Confidence: ✅ HIGH** — Bevestigd in officiële API docs EN Python/PHP SDKs.

**⚠️ KRITIEK: Account Code heeft leading spaties!**

Uit de officiële API documentatie:
> "Code: Unique key, **fixed length numeric string with leading spaces, length 18**. IMPORTANT: When you use OData $filter on this field you have to make sure the filter parameter contains the leading spaces"

**Bron:** https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=CRMAccounts

**Voorbeeld query:**
```http
GET /api/v1/{division}/crm/Accounts?$filter=Code eq '              1234'&$select=ID,Code,Name
```
Let op: Code `1234` wordt gepadded naar 18 karakters: `'              1234'` (14 spaties + 4 cijfers)

**Python SDK implementatie (bevestigt padding):**
```python
def _remote_relation_code(self, code):
    return u"'%18s'" % (code.replace("'", "''"),)
    #       ^^^^^ — left-pad met spaties naar 18 chars!
```

**Response formaat (OData v2):**
```json
{
  "d": {
    "results": [
      {
        "ID": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
        "Code": "              1234",
        "Name": "Klantnaam BV"
      }
    ]
  }
}
```

**Relevante velden:**
| Veld | Type | Filterbaar | Beschrijving |
|------|------|------------|--------------|
| `ID` | Edm.Guid | ✅ | Primary key (de GUID die je nodig hebt) |
| `Code` | Edm.String | ✅ | Unique key, 18-char met leading spaties |
| `Name` | Edm.String | ✅ | Accountnaam (verplicht bij aanmaken) |
| `SearchCode` | Edm.String | ✅ | Zoekcode |
| `Status` | Edm.String | ✅ | Klant status |

### 2.2 Item (Artikel) Opzoeken op Code

**Confidence: ✅ HIGH** — Bevestigd in officiële API docs en Python SDK.

**BELANGRIJK: Item Code heeft GEEN padding!** Anders dan Account Code wordt Item Code gewoon als string gebruikt.

**Voorbeeld query:**
```http
GET /api/v1/{division}/logistics/Items?$filter=Code eq 'ITEMCODE123'&$select=ID,Code,Description
```

**Python SDK implementatie (bevestigt: geen padding):**
```python
def filter(self, code=None, **kwargs):
    if code is not None:
        self._filter_append(kwargs, f"Code eq '{code}'")
    return super().filter(**kwargs)
```

**Bron:** https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=LogisticsItems

**Relevante velden:**
| Veld | Type | Filterbaar | Verplicht | Beschrijving |
|------|------|------------|-----------|--------------|
| `ID` | Edm.Guid | ✅ | Nee | Primary key (de GUID die je nodig hebt) |
| `Code` | Edm.String | ✅ | **Ja** | Artikelcode (uniek per divisie) |
| `Description` | Edm.String | ✅ | **Ja** | Omschrijving |
| `Barcode` | Edm.String | ✅ | Nee | Barcode (numerieke string) |
| `IsSalesItem` | Edm.Byte | ✅ | Nee | Is verkoopartikel |
| `IsPurchaseItem` | Edm.Byte | ✅ | Nee | Is inkoopartikel |

### 2.3 KRITIEK: ItemCode is READ-ONLY in Orderregels

**Confidence: ✅ HIGH** — Bevestigd in API metadata, Go SDK, n8n field configs, en officiële docs.

Dit is de **BELANGRIJKSTE vinding** van het onderzoek:

**`ItemCode` heeft `"post": false, "put": false` in de API metadata.** Je KUNT GEEN orders aanmaken met artikelcodes — je MOET de Item GUID gebruiken.

**Uit ExactOnlineRestApiReference metadata:**

| Veld | Type | POST | PUT | Verplicht |
|------|------|------|-----|-----------|
| **`Item`** | Edm.Guid | ✅ Ja | ✅ Ja | **JA** |
| **`ItemCode`** | Edm.String | ❌ Nee | ❌ Nee | Nee |

Dit geldt voor ZOWEL `SalesOrderLines` ALS `PurchaseOrderLines`.

**Eveneens read-only:**
- `OrderedByName` → Gebruik `OrderedBy` (GUID) in plaats daarvan
- `SupplierCode` → Gebruik `Supplier` (GUID) in plaats daarvan
- `SupplierName` → Read-only

**Implicatie:** Alle ~200K item codes en ~200 account codes moeten EERST naar GUIDs worden omgezet voordat orders kunnen worden aangemaakt.

**Bron:** https://github.com/DannyvdSluijs/ExactOnlineRestApiReference (`meta-data.json`)

### 2.4 Aanbevolen Architectuur: GUID Cache

Gezien de volumes (~200 accounts, ~200K items) is een lokale GUID cache de enige haalbare aanpak:

**Stap 1: Initiële cache vulling**
| Dataset | Endpoint | Records/pagina | Geschatte pagina's | Geschatte tijd |
|---------|----------|----------------|--------------------|----------------|
| Accounts (~200) | `/api/v1/{div}/bulk/CRM/Accounts?$select=ID,Code` | 1.000 | 1 | <1 minuut |
| Items (~200K) | `/api/v1/{div}/bulk/Logistics/Items?$select=ID,Code` | 1.000 | ~200 | ~2 minuten |

**Stap 2: Dagelijkse synchronisatie**
Gebruik sync endpoints voor incremental updates:
```
GET /api/v1/{division}/sync/Logistics/Items?$select=ID,Code
GET /api/v1/{division}/sync/CRM/Accounts?$select=ID,Code
```

**Stap 3: Order aanmaak flow**
```
1. Resolve OrderedBy/Supplier code → Account GUID (uit cache)
2. Per orderregel: Resolve Item code → Item GUID (uit cache)
3. POST /api/v1/{division}/salesorder/SalesOrders met GUIDs
4. Parse response voor OrderID
```

**Geen SDK implementeert lokale caching** — elke lookup gaat direct naar de API. SUPWISE moet dit zelf bouwen.

### 2.5 Paginatie

**Confidence: ✅ HIGH** — Bevestigd in officiële API docs en alle SDKs.

| Methode | Paginagrootte | Bron |
|---------|---------------|------|
| Standaard endpoints | **60** | Officiële docs + n8n default |
| Bulk endpoints | **1.000** | Officiële docs |
| Sync endpoints | **1.000** | Officiële docs |

**Bron:** https://start.exactonline.nl/docs/HlpRestAPIResources.aspx
> "Most of the REST API have a page size of 60. The bulk and sync endpoints have a pagesize of 1000."

**Paginatie werkt via `d.__next` links (OData v2):**
```json
{
  "d": {
    "results": [ ... ],
    "__next": "https://start.exactonline.nl/api/v1/{division}/crm/Accounts?$skiptoken=guid'xxx'"
  }
}
```

Alle SDKs volgen dezelfde aanpak: controleer of `__next` aanwezig is → haal volgende pagina op → herhaal.

**Go SDK voorbeeld:**
```go
var next = f.NextPage
for next != nil {
    _, l, rErr := c.NewRequestAndDo(ctx, "GET", next.String(), nil, &i)
    s = append(s, i...)
    next = l.NextPage
}
```

**⚠️ Maximum `$top` waarde:** Niet gedocumenteerd. SDKs gebruiken `$top=1` voor individuele lookups. Vraag aan Exact.

### 2.6 Batch/Bulk Operaties

**Confidence: ✅ HIGH**

**Bulk endpoints zijn alleen voor LEZEN (GET-only):**
| Endpoint | URI | Paginagrootte |
|----------|-----|---------------|
| Bulk Accounts | `/api/v1/{division}/bulk/CRM/Accounts` | 1.000 |
| Bulk Items | `/api/v1/{division}/bulk/Logistics/Items` | 1.000 |
| Bulk SalesOrders | `/api/v1/{division}/bulk/SalesOrder/SalesOrders` | 1.000 |
| Bulk SalesOrderLines | `/api/v1/{division}/bulk/SalesOrder/SalesOrderLines` | 1.000 |

**Bron:** https://start.exactonline.nl/docs/HlpRestAPIResources.aspx

**Geen bulk POST/create:**
- ❌ Geen `$batch` endpoint gevonden in enige SDK of documentatie
- ❌ Bulk en Sync endpoints zijn alleen GET
- ✅ Orders moeten **één voor één** worden aangemaakt via standaard POST endpoints

**OData `$filter` met `or` operator:**
- ⚠️ Theoretisch ondersteund door OData v2 spec, maar niet expliciet bevestigd in Exact documentatie
- Vraag aan Exact: werkt `$filter=Code eq 'A' or Code eq 'B'`?

### 2.7 Rate Limits

**⚠️ DISCREPANTIE TUSSEN BRONNEN:**

| Bron | Per minuut | Per dag |
|------|------------|---------|
| Community kennis (PHP SDK) | 60/min | 5.000/dag |
| Python SDK response headers | **100/min** | **9.000/dag** |

**Waarschijnlijk tier-afhankelijk. VRAAG AAN EXACT IN MEETING.**

**Rate limit headers (bevestigd in PHP + Python SDKs):**
| Header | Beschrijving |
|--------|-------------|
| `X-RateLimit-Limit` | Dagelijks limiet totaal |
| `X-RateLimit-Remaining` | Dagelijks limiet resterend |
| `X-RateLimit-Reset` | Dagelijks limiet reset timestamp (ms) |
| `X-RateLimit-Minutely-Limit` | Per-minuut limiet totaal |
| `X-RateLimit-Minutely-Remaining` | Per-minuut limiet resterend |
| `X-RateLimit-Minutely-Reset` | Per-minuut limiet reset timestamp (ms) |

**Impact op SUPWISE (worst case: 60/min, 5.000/dag):**
- Initiële item cache (200K items via bulk): 200 requests → 1 dag budget van 5.000 is ruim voldoende
- Dagelijkse order verwerking: afhankelijk van ordervolume
- Elke order = 1 POST request. Met 60/min max = 3.600 orders/uur theoretisch

**Rate limit handling best practice (uit Python SDK):**
```python
class RateLimiter(object):
    def backoff(self):
        seconds = self._should_wait()
        if seconds > 0:
            self.wait(seconds)  # Blokkeert met time.sleep()
            return True
        return False
```

### 2.8 Vragen voor het Meeting — GUID Resolutie & Rate Limits

1. **Is er een manier om orders aan te maken met ItemCode** in plaats van Item GUID?
2. **Werkt `$filter=Code eq 'A' or Code eq 'B'`** voor meerdere items tegelijk opzoeken?
3. **Wat is de maximale `$top` waarde** voor standaard endpoints?
4. **Wat zijn de huidige rate limits?** (Wij zien 60/min OF 100/min, 5.000/dag OF 9.000/dag — welke is correct?)
5. **Zijn rate limits per app, per divisie, of per bedrijf?**
6. **Is er een hogere rate limit tier beschikbaar** voor integratiepartners?
7. **Is er een `$batch` endpoint** voor meerdere operaties in één request?
8. **Wat is de aanbevolen strategie** voor het synchroniseren van ~200K items?

---

## 3. Testomgeving / Sandbox

### 3.1 Geen Dedicated Sandbox Gevonden

**Confidence: ⚠️ MEDIUM** — Geen bewijs gevonden in officiële docs of SDKs.

- ❌ Geen apart test-URL gevonden (geen `test.exactonline.nl` of vergelijkbaar)
- ❌ De API base URL is `https://start.exactonline.nl` voor alle omgevingen
- ❌ Geen enkele SDK refereert naar een sandbox URL
- De Picqer SDK bevat alleen productie-URLs per land:

| Regio | Base URL |
|-------|----------|
| Nederland | `https://start.exactonline.nl` |
| Duitsland | `https://start.exactonline.de` |
| België | `https://start.exactonline.be` |
| UK | `https://start.exactonline.co.uk` |
| USA | `https://start.exactonline.com` |
| Spanje | `https://start.exactonline.es` |
| Frankrijk | `https://start.exactonline.fr` |

### 3.2 Opties voor Testen

**Op basis van community kennis:**

1. **Demo-bedrijf (demonstratiebedrijf):** Exact Online biedt typisch een demo-bedrijf aan bij een nieuw account of proefabonnement, met voorbeelddata.
2. **Kopie-divisie:** Maak een kopie van een bestaande divisie (administratie) vanuit de Exact Online UI. De kopie krijgt een eigen divisiecode en data.

**Divisie-ontdekking (bevestigd in officiële docs):**
```http
GET /api/v1/current/Me?$select=CurrentDivision
GET /api/v1/{division}/system/Divisions
GET /api/v1/{division}/system/AllDivisions
```

**Bron:** https://start.exactonline.nl/docs/HlpRestAPIResources.aspx

### 3.3 Vragen voor het Meeting — Testomgeving

1. **Is er een sandbox of test omgeving beschikbaar** voor API ontwikkeling?
2. **Kunnen we een kopie-divisie maken** specifiek voor API testen?
3. **Is er een demo-bedrijf met voorbeelddata** beschikbaar?
4. **Heeft een testomgeving aparte rate limits** of deelt het met productie?
5. **Is er een manier om testdata te genereren** (artikelen, relaties, etc.)?

---

## 4. Concrete Code Voorbeelden

### 4.1 SalesOrder Aanmaken (POST)

**Endpoint:** `POST /api/v1/{division}/salesorder/SalesOrders`

**Headers:**
```http
Content-Type: application/json
Authorization: Bearer {access_token}
Prefer: return=representation
```

**Minimale body:**
```json
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

**Volledige body (met veelgebruikte optionele velden):**
```json
{
  "OrderedBy": "4f4f8200-77d5-4a70-b743-7f5c68b0a6d7",
  "OrderDate": "2026-04-07T00:00:00Z",
  "DeliveryDate": "2026-04-14T00:00:00Z",
  "Description": "Order voor Klant ABC",
  "YourRef": "PO-12345",
  "Remarks": "Spoedlevering gevraagd",
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

**Verplichte velden (header):**
| Veld | Type | POST | PUT | Beschrijving |
|------|------|------|-----|-------------|
| `OrderedBy` | Edm.Guid | ✅ | ❌ **POST-only!** | Klant Account GUID |
| `SalesOrderLines` | Collection | ✅ | — | Array van orderregels |

**Verplichte velden (orderregel):**
| Veld | Type | POST | PUT | Beschrijving |
|------|------|------|-----|-------------|
| `Item` | Edm.Guid | ✅ | ✅ | **VERPLICHT** — Item GUID |
| `ItemCode` | Edm.String | ❌ | ❌ | **READ-ONLY** |

**⚠️ Let op: `OrderedBy` is POST-only** — je kunt de klant niet wijzigen op een bestaande order. Controleer de klant-GUID goed voor aanmaken!

**Tip:** Gebruik `Prefer: return=representation` header om het aangemaakte object terug te krijgen in de response (bevestigd in n8n SDK).

**Bron:** https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrders

### 4.2 PurchaseOrder Aanmaken (POST)

**Endpoint:** `POST /api/v1/{division}/purchaseorder/PurchaseOrders`

**Minimale body:**
```json
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

**Volledige body:**
```json
{
  "Supplier": "5a6b7c8d-9e0f-1234-5678-9abcdef01234",
  "OrderDate": "2026-04-07T00:00:00Z",
  "ReceiptDate": "2026-04-21T00:00:00Z",
  "Description": "Inkoop bij Leverancier XYZ",
  "YourRef": "SO-67890",
  "Remarks": "Standaard maandelijkse bestelling",
  "Warehouse": "d1e2f3a4-b5c6-7890-abcd-ef1234567890",
  "PurchaseOrderLines": [
    {
      "Item": "a3b2c1d0-e4f5-6789-abcd-ef0123456789",
      "Description": "Grondstof A",
      "QuantityInPurchaseUnits": 100.0,
      "UnitPrice": 12.75
    }
  ]
}
```

**⚠️ KRITIEK VERSCHIL met SalesOrderLines:**
- PurchaseOrderLine gebruikt **`QuantityInPurchaseUnits`** (POST-baar)
- **NIET `Quantity`** — dat is READ-ONLY voor PurchaseOrderLines!
- `Supplier` is ook POST-only (niet wijzigbaar via PUT)

**Verplichte velden (header):**
| Veld | Type | POST | PUT | Beschrijving |
|------|------|------|-----|-------------|
| `Supplier` | Edm.Guid | ✅ | ❌ **POST-only!** | Leverancier Account GUID |
| `PurchaseOrderLines` | Collection | ✅ | — | Array van orderregels |

**Verplichte velden (orderregel):**
| Veld | Type | POST | PUT | Beschrijving |
|------|------|------|-----|-------------|
| `Item` | Edm.Guid | ✅ | ✅ | **VERPLICHT** — Item GUID |
| `QuantityInPurchaseUnits` | Edm.Double | ✅ | ✅ | **VERPLICHT** — Aantal |
| `Quantity` | Edm.Double | ❌ | ❌ | **READ-ONLY** |
| `ItemCode` | Edm.String | ❌ | ❌ | **READ-ONLY** |

**Bron:** https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrders

### 4.3 Account Opzoeken op Code

```http
GET /api/v1/{division}/crm/Accounts?$filter=Code eq '              1234'&$select=ID,Code,Name
Authorization: Bearer {access_token}
```

**⚠️ Let op de 18-karakter padding met leading spaties!**

### 4.4 Item Opzoeken op Code

```http
GET /api/v1/{division}/logistics/Items?$filter=Code eq 'ITEMCODE123'&$select=ID,Code,Description
Authorization: Bearer {access_token}
```

**Geen padding nodig voor Item Code.**

### 4.5 TypeScript Helper: Account Code Padding

```typescript
function padAccountCode(code: string): string {
    return code.padStart(18, ' ');
}

// Gebruik:
const filter = `Code eq '${padAccountCode('1234')}'`;
// Resultaat: Code eq '              1234'
```

### 4.6 TypeScript Helper: Token Refresh met Mutex

```typescript
class ExactTokenManager {
    private refreshing = false;

    async ensureValidToken(): Promise<string> {
        if (this.isTokenExpired()) {
            if (this.refreshing) {
                // Wacht tot andere refresh klaar is
                await this.waitForRefresh();
                return this.accessToken;
            }
            this.refreshing = true;
            try {
                const response = await fetch(
                    'https://start.exactonline.nl/api/oauth2/token',
                    {
                        method: 'POST',
                        headers: {
                            'Content-Type': 'application/x-www-form-urlencoded',
                        },
                        body: new URLSearchParams({
                            grant_type: 'refresh_token',
                            client_id: this.clientId,
                            client_secret: this.clientSecret,
                            refresh_token: this.refreshToken,
                        }).toString(),
                    }
                );
                const data = await response.json();
                this.accessToken = data.access_token;
                this.refreshToken = data.refresh_token; // NIEUWE token opslaan!
                this.expiresAt = Date.now() + 570_000; // 9.5 min
                await this.persistTokens(); // Atomisch naar database
            } finally {
                this.refreshing = false;
            }
        }
        return this.accessToken;
    }
}
```

### 4.7 Bulk Items Ophalen voor GUID Cache

```http
GET /api/v1/{division}/bulk/Logistics/Items?$select=ID,Code
Authorization: Bearer {access_token}
```

Response (1.000 items per pagina):
```json
{
  "d": {
    "results": [
      { "ID": "guid-1", "Code": "ITEM001" },
      { "ID": "guid-2", "Code": "ITEM002" }
    ],
    "__next": "https://start.exactonline.nl/api/v1/{division}/bulk/Logistics/Items?$select=ID,Code&$skiptoken=guid'...'"
  }
}
```

Volg `d.__next` links totdat er geen `__next` meer is.

---

## 5. Gotchas & Risico's

| # | Gotcha | Impact | Oplossing | Bevestigd door |
|---|--------|--------|-----------|----------------|
| 1 | **ItemCode is READ-ONLY in POST/PUT** | 🔴 BREAKING | Bouw GUID cache, resolve vóór aanmaken | API metadata + alle SDKs |
| 2 | **Account Code 18-char padding met spaties** | 🔴 BREAKING | `code.padStart(18, ' ')` | Officiële docs + Python/PHP SDK |
| 3 | **Token endpoint vereist form-urlencoded** | 🔴 BREAKING | Geen JSON body! Gebruik `URLSearchParams` | Alle 5 SDKs |
| 4 | **PurchaseOrderLine: `QuantityInPurchaseUnits` ipv `Quantity`** | 🔴 BREAKING | `Quantity` is read-only voor PO regels | API metadata + n8n |
| 5 | **Refresh token is single-use** | 🔴 BREAKING | Sla ALTIJD nieuwe refresh token op, gebruik mutex | Alle SDKs |
| 6 | **`OrderedBy` is POST-only (niet wijzigbaar via PUT)** | 🟡 DESIGN | Controleer klant-GUID goed vóór aanmaken order | API metadata |
| 7 | **`Supplier` is POST-only (niet wijzigbaar via PUT)** | 🟡 DESIGN | Controleer leverancier-GUID goed vóór aanmaken order | API metadata |
| 8 | **Geen bulk POST voor orders** | 🟡 PERFORMANCE | Eén order per keer, respecteer rate limits | Alle SDKs + docs |
| 9 | **Rate limits zijn onduidelijk** | 🟡 RISICO | Track headers, bouw proactieve backoff | Discrepantie tussen bronnen |
| 10 | **Refresh token verloopt na ~30 dagen inactiviteit** | 🟡 RISICO | Zorg voor minimaal 1 API call per 30 dagen | Community kennis |

---

## 6. Meeting Agenda — Voorgestelde Vragen

### Autorisatie
1. **Is er een service account / API key optie** voor server-to-server integratie zonder browser login?
2. **Wat is de exacte levensduur van de refresh token?** (Wij schatten ~30 dagen maar dit is nergens officieel gedocumenteerd)
3. **Kunnen we een private app registreren** (niet gepubliceerd in App Center) voor interne integratie?
4. **Is er een manier om opnieuw te autoriseren zonder eindgebruiker** als het refresh token verloopt?

### GUID Resolutie
5. **Is er een manier om orders aan te maken met ItemCode** in plaats van Item GUID? (De API metadata zegt dat ItemCode niet POST-baar is)
6. **Werkt `$filter=Code eq 'A' or Code eq 'B'`** voor het opzoeken van meerdere items/accounts tegelijk?
7. **Wat is de maximale `$top` waarde** voor standaard en bulk endpoints?
8. **Wat is de aanbevolen strategie** voor het initieel laden van ~200K items voor een GUID cache?

### Rate Limits
9. **Wat zijn de huidige rate limits?** (Wij zien tegenstrijdige informatie: 60/min of 100/min? 5.000/dag of 9.000/dag?)
10. **Zijn de rate limits per app, per divisie, of per bedrijf?**
11. **Is er een hogere rate limit tier beschikbaar** voor integratiepartners?
12. **Wat is het HTTP statuscode bij rate limiting?** (Wij nemen aan 429, maar dit is niet bevestigd)

### Testomgeving
13. **Is er een sandbox of test omgeving beschikbaar** voor API ontwikkeling?
14. **Kunnen we een kopie-divisie maken** specifiek voor API testen?
15. **Is er een demo-bedrijf met voorbeelddata** dat we kunnen gebruiken?
16. **Heeft een testomgeving aparte rate limits** of deelt het met productie?

### Architectuur & Best Practices
17. **Wat is jullie aanbeveling voor token opslag** in een server-side NestJS applicatie?
18. **Is er een webhook/push mechanisme** voor wijzigingen in items of relaties?
19. **Zijn er plannen voor een v2 API** of nieuwe authenticatieopties?

---

## 7. Referenties

### Officiële Documentatie

| Bron | URL | Status |
|------|-----|--------|
| API Overzicht | https://start.exactonline.nl/docs/HlpRestAPIResources.aspx | ✅ Toegankelijk |
| SalesOrders | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrders | ✅ Toegankelijk |
| SalesOrderLines | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrderLines | ✅ Toegankelijk |
| PurchaseOrders | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrders | ✅ Toegankelijk |
| PurchaseOrderLines | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrderLines | ✅ Toegankelijk |
| Accounts (CRM) | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=CRMAccounts | ✅ Toegankelijk |
| Items (Logistics) | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=LogisticsItems | ✅ Toegankelijk |
| Developer Portal | https://developer.exactonline.com/ | ❌ JS-rendered, niet scrapbaar |
| Support Knowledge Base | https://support.exactonline.com/community/s/knowledge-base | ❌ JS-rendered (Salesforce) |

### SDK Broncode

| SDK | Taal | URL | Nut |
|-----|------|-----|-----|
| Picqer | PHP | https://github.com/picqer/exact-php-client | OAuth flow, rate limits, code padding, meest volwassen |
| ossobv | Python | https://github.com/ossobv/exactonline | Token management, rate limiter, item/relation lookups, INI config |
| alexander-schillemans | Python | https://github.com/alexander-schillemans/python-exact-online | Simpelere wrapper, token renewal |
| Quantix ICT | Node.js/TS | npm `@quantix-ict/exact-online` | TypeScript OAuth, file-based token storage, mutex |
| jellekralt | Node.js | https://github.com/AanZee/node-exact-online | Oudere Node.js wrapper, form-encoded token requests |
| mcnijman | Go | https://github.com/mcnijman/go-exactonline | Auto-generated types, order struct definities |
| n8n (bramknuever) | Node.js | npm `n8n-nodes-exact-online` | n8n community integration, field configs |
| n8n (datafix) | Node.js | npm `@datafix/n8n-nodes-exact-online` | Verbeterde n8n integration, rate limit handling |
| API Metadata | JSON | https://github.com/DannyvdSluijs/ExactOnlineRestApiReference | POST/PUT/GET veld permissies — **autoritative bron** |

---

## Bijlage: SalesOrderLine Schrijfbare Velden (Volledig)

| Veld | Type | POST | PUT | Notities |
|------|------|------|-----|----------|
| Item | Edm.Guid | ✅ | ✅ | **Verplicht** — Item GUID |
| OrderID | Edm.Guid | ✅ | ✅ | **Verplicht** bij losse regel aanmaak |
| Quantity | Edm.Double | ✅ | ✅ | Aantal items |
| UnitPrice | Edm.Double | ✅ | ✅ | Prijs per eenheid |
| NetPrice | Edm.Double | ✅ | ✅ | Netto prijs (alternatief voor UnitPrice) |
| Description | Edm.String | ✅ | ✅ | Regelomschrijving |
| Discount | Edm.Double | ✅ | ✅ | Korting (fractie) |
| VATCode | Edm.String | ✅ | ✅ | BTW code |
| DeliveryDate | Edm.DateTime | ✅ | ✅ | Leverdatum per regel |
| Notes | Edm.String | ✅ | ✅ | Notities |
| ItemCode | Edm.String | ❌ | ❌ | **READ-ONLY** |
| ItemDescription | Edm.String | ❌ | ❌ | **READ-ONLY** |

## Bijlage: PurchaseOrderLine Schrijfbare Velden (Volledig)

| Veld | Type | POST | PUT | Notities |
|------|------|------|-----|----------|
| Item | Edm.Guid | ✅ | ✅ | **Verplicht** — Item GUID |
| PurchaseOrderID | Edm.Guid | ✅ | ❌ | **Verplicht** — Bovenliggende order |
| QuantityInPurchaseUnits | Edm.Double | ✅ | ✅ | **Verplicht** — Gebruik DIT veld |
| UnitPrice | Edm.Double | ✅ | ✅ | Prijs per inkoopeenheid |
| NetPrice | Edm.Double | ✅ | ✅ | Netto prijs |
| Description | Edm.String | ✅ | ✅ | Regelomschrijving |
| Discount | Edm.Double | ✅ | ✅ | Korting |
| Quantity | Edm.Double | ❌ | ❌ | **READ-ONLY** — Gebruik QuantityInPurchaseUnits |
| ItemCode | Edm.String | ❌ | ❌ | **READ-ONLY** |
