# Exact Online API — Research Guide voor Testdivisie

**Doel:** Gestructureerde handleiding om de Exact Online API te onderzoeken en testen  
**Aangemaakt:** 2026-04-07  
**Status:** 🔍 Te verifiëren — gebruik dit document als checklist bij het testen  
**Context:** Na meeting met Exact Online PM & API developer — gecorrigeerde informatie verwerkt

---

## ⚠️ Belangrijke correcties t.o.v. eerdere research

Onze eerdere API research (gebaseerd op officiële docs + 5 SDKs) ging uit van **OAuth2 Authorization Code flow**. De meeting heeft het volgende gecorrigeerd:

| Onderwerp | Eerdere research | Na meeting |
|-----------|-------------------|------------|
| **Authenticatie** | OAuth2 (enige optie) | API key via privé app-registratie |
| **Testomgeving** | Niet gevonden | Testdivisie beschikbaar |
| **Sync methode** | Bulk endpoints + GUID cache | Standaard POST/PUT per entity |
| **Rate limits** | 60/min of 100/min (onduidelijk) | Hoog volume OK bij initiële sync |
| **Items in testdivisie** | N.v.t. | Moeten opnieuw aangemaakt worden |

De rest van onze research (endpoint structuur, veldnamen, GUID-vereisten, Account Code padding) is nog steeds geldig en bevestigd door de meeting.

---

## Hoe dit document te gebruiken

1. **Onderwerpen A–E** behandelen elk een kennislacune
2. Per onderwerp staat er:
   - ✅ **Wat we denken te weten** — gebaseerd op meeting + research
   - ❓ **Wat we moeten verifiëren** — concrete vragen
   - 🧪 **Test-requests** — curl-commando's om uit te voeren zodra je API-toegang hebt
3. **Variabelen** die je overal moet invullen:
   - `{{API_KEY}}` — de API key die je krijgt bij app-registratie
   - `{{DIVISION}}` — de divisie-ID van de testdivisie (waarschijnlijk een integer)
   - `{{BASE_URL}}` — `https://start.exactonline.nl` (NL regio)

---

## A. API Key Authenticatie

### ✅ Wat we denken te weten

- Exact biedt API key authenticatie aan voor **privé geregistreerde apps** (niet gepubliceerd in het App Center)
- Dit vervangt de OAuth2 Authorization Code flow die alle publieke SDKs gebruiken
- De API key wordt verkregen via app-registratie in het Exact App Center

### ❓ Wat we moeten verifiëren

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| A1 | **Wat is het exacte formaat van de Authorization header?** Is het `Authorization: Bearer {{API_KEY}}`? Of een ander formaat zoals `Authorization: ApiKey {{API_KEY}}`? Of een custom header zoals `X-Api-Key`? | Zonder het juiste headerformaat werkt geen enkel request |
| A2 | **Waar registreer je de app precies?** URL van het Exact App Center portaal — is dat `https://apps.exactonline.com`? Of een ander portaal? | Nodig om het proces te starten |
| A3 | **Wat krijg je terug bij registratie?** Een enkele API key string? Of een `client_id` + `client_secret` combinatie? Of iets anders? | Bepaalt hoe we credentials opslaan |
| A4 | **Is de API key permanent of heeft die een expiry?** Moet de key periodiek vernieuwd worden? | Bepaalt of we token-refresh logica nodig hebben |
| A5 | **Is de API key gebonden aan een specifieke divisie?** Of werkt één key voor alle divisies binnen het account? | Bepaalt of we meerdere keys moeten beheren |
| A6 | **Kan de API key gerevoked worden?** En zo ja, hoe? Via het portaal of via een API call? | Nodig voor security-beleid |

### 🧪 Test-requests

**Test 1: Probeer Bearer token authenticatie**
```bash
curl -v \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/current/Me"
```

**Test 2: Als Bearer niet werkt, probeer andere formaten**
```bash
# Optie A: Custom header
curl -v \
  -H "X-Api-Key: {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/current/Me"

# Optie B: Basic auth met client_id:client_secret
curl -v \
  -u "{{CLIENT_ID}}:{{CLIENT_SECRET}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/current/Me"
```

**Test 3: Controleer welke response headers je terugkrijgt**
```bash
# Let specifiek op deze headers in de response:
# - X-RateLimit-Limit
# - X-RateLimit-Remaining
# - X-RateLimit-Minutely-Limit
# - X-RateLimit-Minutely-Remaining
# - Content-Type (zou application/json moeten zijn)
```

**Wat je verwacht bij succesvolle auth:**
```json
{
  "d": {
    "results": [
      {
        "CurrentDivision": 123456,
        "FullName": "Naam van de gebruiker",
        "UserID": "guid-hier"
      }
    ]
  }
}
```

**Wat je verwacht bij gefaalde auth:**
- HTTP 401 Unauthorized
- Noteer de exacte error response body — die hebben we nodig voor error handling

### 📋 Vragen voor Exact technisch contact

> "We hebben begrepen dat we een API key krijgen via een privé app-registratie. Kunnen jullie ons doorlopen hoe dat proces werkt? Specifiek: waar registreren we, wat krijgen we terug, en hoe zetten we de key in onze API requests?"

---

## B. REST Endpoints — Request/Response Formaten

### B0. Divisie ophalen

Voordat je iets anders kunt testen, heb je de divisie-ID nodig.

**✅ Wat we weten:**
- `/api/v1/current/Me` retourneert `CurrentDivision`
- De divisie-ID is waarschijnlijk een **integer** (niet een GUID)
- Bron: Officiële API docs + alle SDKs

**🧪 Test-request:**
```bash
curl -s \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/current/Me?$select=CurrentDivision,FullName,UserID" \
  | python3 -m json.tool
```

**Verwachte response:**
```json
{
  "d": {
    "results": [
      {
        "CurrentDivision": 123456,
        "FullName": "Naam",
        "UserID": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
      }
    ]
  }
}
```

**Alle divisies ophalen:**
```bash
curl -s \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/system/Divisions?$select=Code,Description,HID" \
  | python3 -m json.tool
```

> ❓ **Noteer:** Is `CurrentDivision` de testdivisie of de productiedivisie? Welke divisie-ID hoort bij de testomgeving?

---

### B1. POST Account (Klant/Leverancier) aanmaken

**✅ Wat we weten (uit research):**
- Endpoint: `POST /api/v1/{{DIVISION}}/crm/Accounts`
- `Name` is verplicht
- `Code` is een 18-karakter string met leading spaties (maar bij aanmaken wordt dit waarschijnlijk automatisch gepadded)
- Klant vs leverancier wordt onderscheiden door `IsSupplier` en/of `IsCustomer` vlaggen
- Response bevat een GUID in het `ID` veld
- Een bedrijf kan ZOWEL klant als leverancier zijn (één Account, twee rollen)
- Bron: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=CRMAccounts

**❓ Wat we moeten verifiëren:**

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| B1.1 | **Welke velden zijn echt verplicht bij POST?** Alleen `Name`? Of ook `Code`? | Bepaalt wat we minimaal moeten meesturen |
| B1.2 | **Wordt `Code` automatisch gegenereerd** als je het niet meestuurt? Of moet je zelf een unieke code opgeven? | In SUPWISE hebben we bestaande numerieke codes |
| B1.3 | **Hoe zet je `IsSupplier` en `IsCustomer`?** Zijn dit booleans (`true`/`false`), bytes (`0`/`1`), of strings? | Fout datatype = 400 error |
| B1.4 | **Wat is het exacte response format** bij succesvolle aanmaak? Komt de GUID terug als `ID` of als `d.ID`? | Nodig om de GUID te parsen en op te slaan |
| B1.5 | **Wat als je een Account aanmaakt met een Code die al bestaat?** Krijg je een 409 Conflict? Of een 400 met een specifieke foutmelding? | Nodig voor idempotent sync design |

**🧪 Test-requests:**

**Test 1: Minimale klant aanmaken**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts" \
  -d '{
    "Name": "SUPWISE Test Klant 001",
    "IsCustomer": true
  }'
```

**Test 2: Minimale leverancier aanmaken**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts" \
  -d '{
    "Name": "SUPWISE Test Leverancier 001",
    "IsSupplier": true
  }'
```

**Test 3: Klant + leverancier (dual role) aanmaken**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts" \
  -d '{
    "Name": "SUPWISE Test Dual Role 001",
    "IsCustomer": true,
    "IsSupplier": true
  }'
```

**Test 4: Account met specifieke Code aanmaken**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts" \
  -d '{
    "Name": "SUPWISE Test Met Code",
    "Code": "99001",
    "IsCustomer": true
  }'
```

**Test 5: Bestaand account opzoeken op Code (let op 18-char padding!)**
```bash
# Pas de padding aan op basis van je code-lengte
# Voorbeeld: code "99001" → 13 spaties + 5 cijfers = 18 karakters
curl -s \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts?\$filter=Code%20eq%20'%20%20%20%20%20%20%20%20%20%20%20%20%2099001'&\$select=ID,Code,Name,IsCustomer,IsSupplier" \
  | python3 -m json.tool
```

**📝 Noteer bij elke test:**
1. HTTP status code
2. Volledige response body (met GUID)
3. Of `Code` automatisch werd ingevuld (en hoe)
4. Of `IsCustomer`/`IsSupplier` als boolean of byte werd geaccepteerd
5. Eventuele rate limit headers

---

### B2. POST Item (Artikel) aanmaken

**✅ Wat we weten (uit research):**
- Endpoint: `POST /api/v1/{{DIVISION}}/logistics/Items`
- `Code` en `Description` zijn verplicht
- Item Code heeft GEEN 18-karakter padding (anders dan Account Code)
- Item Code is uniek per divisie
- Response bevat een GUID in het `ID` veld
- Bron: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=LogisticsItems

**❓ Wat we moeten verifiëren:**

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| B2.1 | **Welke velden zijn minimaal verplicht?** `Code` + `Description`? Of ook `Unit`? | Bepaalt minimum payload |
| B2.2 | **Hoe werkt het `Unit` veld?** Is dat een vrije string ("pc", "kg") of een GUID van een unit-definitie? | SUPWISE gebruikt: pc, kg, ltr, m, m², m³, set, pair — moeten die gemapt worden? |
| B2.3 | **Waar vind je de beschikbare units?** Is er een endpoint om units op te halen? (bijv. `/api/v1/{{DIVISION}}/logistics/Units`) | Nodig voor de mapping |
| B2.4 | **Bestaat `IsSalesItem` / `IsPurchaseItem`?** En zijn die standaard `true`? | Een item moet als verkoop- EN inkoopitem beschikbaar zijn |
| B2.5 | **Wat als Code al bestaat?** Foutmelding of silent overwrite? | Idempotent sync design |

**🧪 Test-requests:**

**Test 1: Minimaal item aanmaken**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/logistics/Items" \
  -d '{
    "Code": "SUPWISE-TEST-001",
    "Description": "Test Item Schroefbouten M10x50"
  }'
```

**Test 2: Item met unit aanmaken (string)**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/logistics/Items" \
  -d '{
    "Code": "SUPWISE-TEST-002",
    "Description": "Test Item Verf 5L",
    "Unit": "ltr",
    "IsSalesItem": true,
    "IsPurchaseItem": true
  }'
```

**Test 3: Beschikbare units ophalen**
```bash
# Probeer het Units endpoint
curl -s \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/logistics/Units?\$select=ID,Code,Description" \
  | python3 -m json.tool
```

**Test 4: Bestaand item opzoeken op Code (geen padding nodig)**
```bash
curl -s \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/logistics/Items?\$filter=Code%20eq%20'SUPWISE-TEST-001'&\$select=ID,Code,Description,Unit" \
  | python3 -m json.tool
```

**📝 Noteer:**
1. Werkt `Unit` als string of als GUID-referentie?
2. Welke units bestaan er al in de testdivisie?
3. Moeten SUPWISE-units (pc, kg, ltr, m, m², m³, set, pair) gemapt worden naar Exact-units?
4. Wat is de default voor `IsSalesItem` / `IsPurchaseItem` als je ze niet meestuurt?

---

### B3. POST SalesOrder (Verkooporder) aanmaken

**✅ Wat we weten (uit research):**
- Endpoint: `POST /api/v1/{{DIVISION}}/salesorder/SalesOrders`
- `OrderedBy` (klant GUID) is verplicht — en POST-only (niet wijzigbaar via PUT!)
- `SalesOrderLines` array is verplicht, inline meegestuurd
- Per orderregel: `Item` (GUID) is verplicht — `ItemCode` is READ-ONLY
- Per orderregel: `Quantity` (Double) is schrijfbaar
- Gebruik `Prefer: return=representation` header om het aangemaakte object terug te krijgen
- Bron: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrders

**❓ Wat we moeten verifiëren:**

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| B3.1 | **Zijn `OrderedBy` en minstens 1 `SalesOrderLine` de enige verplichte velden?** Of is `OrderDate` ook verplicht? | Minimale payload bepalen |
| B3.2 | **Hoe refereer je naar Items in de orderregels?** Bevestig dat `Item` (GUID) het enige schrijfbare veld is en dat `ItemCode` echt read-only is | Cruciaal voor de hele sync architectuur |
| B3.3 | **Werkt `Prefer: return=representation`?** En wat krijg je exact terug? Alleen de header of ook alle regels? | Bepaalt of we een extra GET nodig hebben na POST |
| B3.4 | **Wordt `OrderNumber` automatisch gegenereerd?** Of moeten we er zelf een opgeven? | In SUPWISE slaan we het Exact ordernummer op |
| B3.5 | **Kun je een `YourRef` (onze referentie) meesturen?** En zo ja, is die doorzoekbaar via `$filter`? | Handig voor het terugvinden van orders |
| B3.6 | **Wat is het datumformaat?** ISO 8601 (`2026-04-07T00:00:00Z`) of OData datetime (`/Date(1234567890)/`)? | Fout formaat = parse error |

**🧪 Test-requests:**

> ⚠️ **Voorwaarde:** Je hebt eerst een Account GUID (klant) en Item GUID nodig. Voer eerst tests B1 en B2 uit.

**Test 1: Minimale verkooporder**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -H "Prefer: return=representation" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/salesorder/SalesOrders" \
  -d '{
    "OrderedBy": "{{KLANT_GUID}}",
    "SalesOrderLines": [
      {
        "Item": "{{ITEM_GUID}}",
        "Quantity": 10.0,
        "UnitPrice": 25.50
      }
    ]
  }'
```

**Test 2: Verkooporder met meerdere regels en optionele velden**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -H "Prefer: return=representation" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/salesorder/SalesOrders" \
  -d '{
    "OrderedBy": "{{KLANT_GUID}}",
    "OrderDate": "2026-04-07T00:00:00Z",
    "DeliveryDate": "2026-04-14T00:00:00Z",
    "Description": "SUPWISE Test Order",
    "YourRef": "SUPWISE-SO-12345",
    "Remarks": "Aangemaakt via SUPWISE sync test",
    "SalesOrderLines": [
      {
        "Item": "{{ITEM_GUID_1}}",
        "Description": "Test Item A",
        "Quantity": 10.0,
        "UnitPrice": 25.50
      },
      {
        "Item": "{{ITEM_GUID_2}}",
        "Description": "Test Item B",
        "Quantity": 5.0,
        "UnitPrice": 100.00
      }
    ]
  }'
```

**Test 3: Verifieer dat ItemCode NIET werkt als alternatief**
```bash
# Dit zou moeten falen of de ItemCode negeren
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/salesorder/SalesOrders" \
  -d '{
    "OrderedBy": "{{KLANT_GUID}}",
    "SalesOrderLines": [
      {
        "ItemCode": "SUPWISE-TEST-001",
        "Quantity": 5.0,
        "UnitPrice": 10.00
      }
    ]
  }'
```

**Verwachte response bij succes:**
```json
{
  "d": {
    "OrderID": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
    "OrderNumber": 12345,
    "OrderedBy": "{{KLANT_GUID}}",
    "OrderDate": "/Date(1712448000000)/",
    "SalesOrderLines": {
      "results": [
        {
          "ID": "yyyyyyyy-yyyy-yyyy-yyyy-yyyyyyyyyyyy",
          "Item": "{{ITEM_GUID}}",
          "Quantity": 10.0,
          "UnitPrice": 25.50
        }
      ]
    }
  }
}
```

**📝 Noteer:**
1. Exact response format (is het `d.OrderID` of `d.results[0].OrderID`?)
2. Wordt `OrderNumber` automatisch gegenereerd?
3. Wordt `YourRef` geaccepteerd en is het doorzoekbaar?
4. Wat doet de API als `ItemCode` i.p.v. `Item` GUID wordt gebruikt?
5. Datumformaat in response — ISO of OData `/Date()/`?

---

### B4. POST PurchaseOrder (Inkooporder) aanmaken

**✅ Wat we weten (uit research):**
- Endpoint: `POST /api/v1/{{DIVISION}}/purchaseorder/PurchaseOrders`
- `Supplier` (leverancier GUID) is verplicht — en POST-only (niet wijzigbaar via PUT!)
- `PurchaseOrderLines` array is verplicht, inline meegestuurd
- **KRITIEK VERSCHIL:** Gebruik `QuantityInPurchaseUnits` (schrijfbaar), NIET `Quantity` (read-only)!
- Per orderregel: `Item` (GUID) is verplicht — `ItemCode` is READ-ONLY
- Bron: https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrders

**❓ Wat we moeten verifiëren:**

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| B4.1 | **Bevestig dat `Quantity` read-only is en `QuantityInPurchaseUnits` gebruikt moet worden** | Dit is een BREAKING verschil met SalesOrderLines |
| B4.2 | **Wat is het verschil tussen `Quantity` en `QuantityInPurchaseUnits`?** Gaat het om conversie (bijv. je koopt in dozen van 12 maar de item-eenheid is stuks)? | Bepaalt welke waarde we uit SUPWISE halen |
| B4.3 | **Bestaat `Warehouse` als veld op PurchaseOrder level?** Welk formaat — GUID of code? | SUPWISE orders hebben een default warehouse |
| B4.4 | **Wordt `PurchaseOrderNumber` automatisch gegenereerd?** | Nodig voor opslag in SUPWISE |
| B4.5 | **Werkt `YourRef` ook op PurchaseOrders?** | Voor koppeling SUPWISE → Exact |

**🧪 Test-requests:**

> ⚠️ **Voorwaarde:** Je hebt een Account GUID (leverancier, `IsSupplier=true`) en Item GUIDs nodig.

**Test 1: Minimale inkooporder**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -H "Prefer: return=representation" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/purchaseorder/PurchaseOrders" \
  -d '{
    "Supplier": "{{LEVERANCIER_GUID}}",
    "PurchaseOrderLines": [
      {
        "Item": "{{ITEM_GUID}}",
        "QuantityInPurchaseUnits": 100.0,
        "UnitPrice": 12.75
      }
    ]
  }'
```

**Test 2: Inkooporder met meerdere regels en optionele velden**
```bash
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -H "Prefer: return=representation" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/purchaseorder/PurchaseOrders" \
  -d '{
    "Supplier": "{{LEVERANCIER_GUID}}",
    "OrderDate": "2026-04-07T00:00:00Z",
    "ReceiptDate": "2026-04-21T00:00:00Z",
    "Description": "SUPWISE Test Inkoop",
    "YourRef": "SUPWISE-PO-67890",
    "Remarks": "Aangemaakt via SUPWISE sync test",
    "PurchaseOrderLines": [
      {
        "Item": "{{ITEM_GUID_1}}",
        "Description": "Grondstof A",
        "QuantityInPurchaseUnits": 100.0,
        "UnitPrice": 12.75
      },
      {
        "Item": "{{ITEM_GUID_2}}",
        "Description": "Onderdeel B",
        "QuantityInPurchaseUnits": 50.0,
        "UnitPrice": 8.00
      }
    ]
  }'
```

**Test 3: Verifieer dat `Quantity` NIET werkt (moet `QuantityInPurchaseUnits` zijn)**
```bash
# Dit zou moeten falen of Quantity negeren
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/purchaseorder/PurchaseOrders" \
  -d '{
    "Supplier": "{{LEVERANCIER_GUID}}",
    "PurchaseOrderLines": [
      {
        "Item": "{{ITEM_GUID}}",
        "Quantity": 100.0,
        "UnitPrice": 12.75
      }
    ]
  }'
```

**📝 Noteer:**
1. Wat gebeurt er als je `Quantity` stuurt i.p.v. `QuantityInPurchaseUnits`?
2. Exact response format en welke velden terugkomen
3. Wordt `PurchaseOrderNumber` automatisch gegenereerd?
4. Is `Warehouse` een veld op PO level? Welk formaat?

---

## C. Rate Limits

### ✅ Wat we denken te weten

- Hoog volume OK bij initiële bulk sync (items + accounts aanmaken)
- Daarna normale rate limits
- Response headers bevatten rate limit info (bevestigd door alle SDKs):
  - `X-RateLimit-Limit` / `X-RateLimit-Remaining` (dagelijks)
  - `X-RateLimit-Minutely-Limit` / `X-RateLimit-Minutely-Remaining` (per minuut)
  - `X-RateLimit-Reset` / `X-RateLimit-Minutely-Reset` (reset timestamps in milliseconden)
- **Tegenstrijdige bronnen:** 60/min of 100/min? 5.000/dag of 9.000/dag?

### ❓ Wat we moeten verifiëren

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| C1 | **Wat zijn de exacte rate limits?** Per minuut en per dag | Bepaalt onze throttling strategie |
| C2 | **Scope:** Per app? Per divisie? Per API key? Per bedrijf? | Bepaalt of test en productie limits delen |
| C3 | **Is er een verschil in limits** voor de testdivisie vs productie? | We willen onbeperkt kunnen testen |
| C4 | **Welke HTTP status bij rate limiting?** 429 Too Many Requests? | Error handling |
| C5 | **Is er een bulk/batch mode** met hogere limits voor initiële sync? | Initieel moeten we ~200K items + ~200 accounts aanmaken |

### 🧪 Test-requests

**Test 1: Rate limit headers bekijken bij een normaal request**
```bash
curl -v \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts?\$top=1" \
  2>&1 | grep -i "x-ratelimit"
```

**Test 2: Burst test — stuur 10 requests snel achter elkaar**
```bash
for i in $(seq 1 10); do
  echo "--- Request $i ---"
  curl -s -o /dev/null -w "HTTP %{http_code}" \
    -H "Authorization: Bearer {{API_KEY}}" \
    -H "Accept: application/json" \
    "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts?\$top=1"
  echo ""
done
```

**📝 Noteer:**
1. Exacte waarden van `X-RateLimit-Limit` en `X-RateLimit-Minutely-Limit`
2. Of de limits dalen na elke request
3. Of je een 429 krijgt na veel requests, en wat de response body dan is

---

## D. Divisie

### ✅ Wat we denken te weten

- Testdivisie is beschikbaar
- Divisie-ID is waarschijnlijk een integer
- Items moeten opnieuw aangemaakt worden in de testdivisie (bestaan daar nog niet)

### ❓ Wat we moeten verifiëren

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| D1 | **Wat is de divisie-ID van de testdivisie?** | Nodig voor alle API calls |
| D2 | **Wat is de divisie-ID van productie?** | Om te weten welke we NIET mogen gebruiken |
| D3 | **Is de testdivisie leeg?** Of staan er al standaard accounts/items in? | Bepaalt of we alles from scratch moeten aanmaken |
| D4 | **Heeft de testdivisie dezelfde instellingen** als productie (BTW, valuta, etc.)? | Verschillen kunnen bugs veroorzaken die pas in productie opvallen |

### 🧪 Test-requests

```bash
# Alle beschikbare divisies ophalen
curl -s \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/system/Divisions?\$select=Code,Description,HID,Status" \
  | python3 -m json.tool

# Of alle divisies inclusief inactieve
curl -s \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/system/AllDivisions?\$select=Code,Description,HID,Status" \
  | python3 -m json.tool
```

---

## E. Error Handling

### ✅ Wat we denken te weten

- Exact gebruikt standaard HTTP statuscodes (200, 201, 400, 401, 404, 429, 500)
- Error responses zijn in JSON (OData v2 formaat)
- Rate limit overschrijding geeft waarschijnlijk een 429

### ❓ Wat we moeten verifiëren

| # | Vraag | Waarom belangrijk |
|---|-------|-------------------|
| E1 | **Hoe ziet een validation error (400) eruit?** Exacte JSON structuur met veldnamen en foutmeldingen | Error parsing in SUPWISE |
| E2 | **Hoe ziet een auth error (401) eruit?** | Detecteren wanneer API key ongeldig is |
| E3 | **Hoe ziet een not-found (404) eruit?** | Wanneer een GUID niet bestaat |
| E4 | **Hoe ziet een rate limit error (429) eruit?** Met Retry-After header? | Retry logica |
| E5 | **Zijn er custom error codes** naast HTTP statuscodes? | Fijnmaziger error handling |

### 🧪 Test-requests

**Test 1: Validation error — verplicht veld mist**
```bash
# Account aanmaken zonder Name (verplicht)
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts" \
  -d '{}'
```

**Test 2: Auth error — ongeldige API key**
```bash
curl -v \
  -H "Authorization: Bearer INVALID_KEY_12345" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/current/Me"
```

**Test 3: Not found — niet-bestaande GUID**
```bash
curl -v \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts(guid'00000000-0000-0000-0000-000000000000')"
```

**Test 4: Duplicate code — zelfde Account Code twee keer**
```bash
# Maak eerst een account aan met Code "99999"
# Probeer dan dezelfde Code opnieuw
curl -v -X POST \
  -H "Authorization: Bearer {{API_KEY}}" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  "{{BASE_URL}}/api/v1/{{DIVISION}}/crm/Accounts" \
  -d '{
    "Name": "Duplicate Test",
    "Code": "99999",
    "IsCustomer": true
  }'
```

**Verwachte error response structuur (gebaseerd op OData v2):**
```json
{
  "error": {
    "code": "",
    "message": {
      "lang": "en-US",
      "value": "Beschrijving van de fout"
    }
  }
}
```

**📝 Noteer per error type:**
1. Exacte HTTP status code
2. Volledige response body
3. Of er een `Retry-After` header is bij 429
4. Of foutmeldingen in het Nederlands of Engels zijn
5. Of er per-veld validatie errors zijn (of alleen een generieke melding)

---

## F. Aanbevolen Testvolgorde

Volg deze volgorde om systematisch alle endpoints te testen:

### Fase 1: Authenticatie & Divisie
1. ✅ Verkrijg API key via app-registratie
2. 🧪 Test auth met `GET /api/v1/current/Me` (test A)
3. 🧪 Haal divisie-ID op (test D)
4. 🧪 Noteer rate limit headers uit response

### Fase 2: Master Data Aanmaken
5. 🧪 Maak test-klant aan (test B1, test 1)
6. 🧪 Maak test-leverancier aan (test B1, test 2)
7. 🧪 Maak dual-role account aan (test B1, test 3)
8. 🧪 Haal beschikbare units op (test B2, test 3)
9. 🧪 Maak 2-3 test-items aan (test B2, tests 1-2)
10. 📝 Noteer alle GUIDs die terugkomen!

### Fase 3: Orders Aanmaken
11. 🧪 Maak minimale verkooporder aan met klant + item GUIDs (test B3, test 1)
12. 🧪 Maak verkooporder met meerdere regels (test B3, test 2)
13. 🧪 Verifieer dat ItemCode NIET werkt (test B3, test 3)
14. 🧪 Maak minimale inkooporder aan (test B4, test 1)
15. 🧪 Verifieer Quantity vs QuantityInPurchaseUnits (test B4, test 3)

### Fase 4: Error Handling
16. 🧪 Test validation error (test E, test 1)
17. 🧪 Test auth error (test E, test 2)
18. 🧪 Test not-found error (test E, test 3)
19. 🧪 Test duplicate code error (test E, test 4)

### Fase 5: Rate Limits & Bulk
20. 🧪 Burst test (test C, test 2)
21. 🧪 Noteer alle rate limit headers

---

## G. Resultaten Template

Kopieer dit template en vul het in na het testen:

```markdown
## Testresultaten — {{DATUM}}

### Auth formaat
- Header: `Authorization: _______________`
- Verkregen via: _______________

### Divisie
- Test divisie ID: _______________
- Productie divisie ID: _______________
- Type (integer/GUID): _______________

### Rate limits (uit response headers)
- X-RateLimit-Limit: _______________
- X-RateLimit-Minutely-Limit: _______________
- Scope: _______________

### Account aanmaken
- Verplichte velden: _______________
- IsCustomer/IsSupplier type (bool/byte): _______________
- Code auto-generatie (ja/nee): _______________
- Response format: (plak response)

### Item aanmaken
- Verplichte velden: _______________
- Unit type (string/GUID): _______________
- Beschikbare units in testdivisie: _______________
- Response format: (plak response)

### SalesOrder aanmaken
- Verplichte velden: _______________
- OrderNumber auto-generatie (ja/nee): _______________
- YourRef geaccepteerd (ja/nee): _______________
- ItemCode als alternatief (ja/nee — verwacht: NEE): _______________
- Response format: (plak response)

### PurchaseOrder aanmaken
- Verplichte velden: _______________
- QuantityInPurchaseUnits vs Quantity: _______________
- Warehouse veld (beschikbaar/formaat): _______________
- Response format: (plak response)

### Error formaten
- 400 Validation error: (plak response)
- 401 Auth error: (plak response)
- 404 Not found: (plak response)
- Duplicate code error: (plak response + status code)

### Onverwachte bevindingen
- _______________
```

---

## H. Referenties

### Officiële API Documentatie
| Pagina | URL |
|--------|-----|
| API Overzicht | https://start.exactonline.nl/docs/HlpRestAPIResources.aspx |
| CRM Accounts | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=CRMAccounts |
| Logistics Items | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=LogisticsItems |
| SalesOrders | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrders |
| SalesOrderLines | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=SalesOrderSalesOrderLines |
| PurchaseOrders | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrders |
| PurchaseOrderLines | https://start.exactonline.nl/docs/HlpRestAPIResourcesDetails.aspx?name=PurchaseOrderPurchaseOrderLines |

### Eerdere Research
| Document | Pad |
|----------|-----|
| API Research (volledig) | `phases/phase-5b/exact-api-research.md` |
| Doc Research Findings | `phases/phase-5b/doc-research-findings.md` |
| SDK Research Findings | `phases/phase-5b/sdk-research-findings.md` |
| Meeting Q&A | `specs/exact-meeting-qa.md` |

### Bekende Gotchas (uit research)
| # | Gotcha | Status na meeting |
|---|--------|-------------------|
| 1 | ItemCode is READ-ONLY in POST/PUT | ⚠️ Te verifiëren |
| 2 | Account Code 18-char padding met spaties | ⚠️ Te verifiëren |
| 3 | PurchaseOrderLine: `QuantityInPurchaseUnits` i.p.v. `Quantity` | ⚠️ Te verifiëren |
| 4 | `OrderedBy` / `Supplier` zijn POST-only (niet wijzigbaar) | ⚠️ Te verifiëren |
| 5 | OAuth2 → API key (GECORRIGEERD door meeting) | ✅ Bevestigd |
| 6 | Geen sandbox → Testdivisie beschikbaar (GECORRIGEERD door meeting) | ✅ Bevestigd |
