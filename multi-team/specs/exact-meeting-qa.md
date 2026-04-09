# Exact Online Meeting — Q&A Voorbereiding

**Datum:** _(invullen)_  
**Aanwezig:** _(invullen — Mihail + contactpersoon Exact / klant)_

---

## 1. Opening — Elevator Pitch

SUPWISE beheert verkoop- en inkooporders voor een marine supplies bedrijf — van klantorder tot inkoop bij leveranciers. We willen die orders doorzetten naar Exact Online zodat de boekhouding automatisch up-to-date is, zonder dubbel invoer. Daarvoor hebben we API-toegang nodig, plus een paar beslissingen over hoe de data tussen de twee systemen gemapped wordt. Het gesprek van vandaag gaat precies daarover.

---

## 2. Wat we al weten

*Laten zien dat we onze huiswerk gedaan hebben.*

- We kennen de API-structuur: SalesOrders endpoint voor verkooporders, PurchaseOrders endpoint voor inkooporders
- 1 SUPWISE order → 1 Exact verkooporder + N Exact inkooporders (één per leverancier — als een order 4 leveranciers heeft, zijn dat 4 aparte POs in Exact)
- We weten dat we OAuth2 authorization code flow nodig hebben voor de koppeling
- We kennen de rate limits: 60 requests per minuut, 5.000 per dag (reset om middernacht)
- We hebben sync-tracking ontworpen: elke order heeft een status (niet gesynchroniseerd, gesynchroniseerd, mislukt, verouderd)
- We hebben ~200.000 artikelen, 24 artikelgroepen, en bestaande numerieke Exact-codes uit een eerdere CSV-import

---

## 3. Vragen over de Exact omgeving

### Welk Exact Online pakket gebruiken jullie?

*De beschikbare API-endpoints verschillen per pakket — dit bepaalt wat we kunnen bouwen.*

Wholesale & Distribution of Manufacturing? Of iets anders?

- **Als het W&D is** → dan is dat precies wat we nodig hebben. Alle SalesOrders en PurchaseOrders endpoints zijn beschikbaar.
- **Als het Manufacturing is** → dan zijn er extra endpoints beschikbaar voor BOM en routing. Dat is meer dan we nodig hebben voor V1, maar goed om te weten voor de toekomst.
- **Als het een ander pakket is** → dan moeten we nakijken welke endpoints beschikbaar zijn — dat kan de scope beïnvloeden.

---

### Is er een sandbox / test omgeving beschikbaar?

*Dit is een showstopper. We kunnen niet ontwikkelen tegen productie.*

"Zonder een testomgeving kunnen we niet beginnen met development — we riskeren anders echte boekingen in jullie productiesysteem."

- **Als er een sandbox is** → goed, we hebben de credentials nodig om te starten.
- **Als er geen sandbox is** → kunnen jullie er een aanmaken? Exact Online biedt testomgevingen aan via het partnerportaal. Als dat niet lukt, moeten we bespreken hoe we dit anderszins oplossen (bijv. een aparte testdivisie in productie met dummy data — niet ideaal, maar bespreekbaar).

---

### Welke divisie(s) gebruiken jullie in Exact?

*Exact werkt met divisies. We hebben de divisie-ID nodig om de juiste API-calls te doen.*

Eén divisie of meerdere? Als er meerdere zijn — pushen we orders naar één centrale divisie, of afhankelijk van iets (bijv. per vestiging)?

---

### Staan klanten, leveranciers en artikelen al in Exact?

*Dit bepaalt of we alleen orders hoeven te pushen, of ook de master data.*

- **Als alles al in Exact staat** → dan hoeven we alleen orders te pushen. Prima.
- **Als het er (nog) niet in staat** → dan moeten we ook klanten, leveranciers en artikelen vanuit SUPWISE kunnen aanmaken in Exact. Dat is meer werk en moet meegenomen worden in de scope.

---

## 4. Vragen over data mapping — ⚠️ KRITISCH

*Dit is het meest belangrijke onderwerp van het gesprek. Goed de tijd voor nemen.*

### Het GUID probleem

In SUPWISE slaan we numerieke codes op voor klanten en leveranciers — bijvoorbeeld '10045' voor een klant, of '8820' voor een leverancier. Die codes komen uit een eerdere CSV-import.

Maar de Exact API werkt niet met die numerieke codes — die werkt met GUIDs. Een GUID ziet er zo uit: `3b4f8a2c-19d7-4e1a-a8b3-...` — een lange unieke identifier. We moeten van onze numerieke code naar de bijbehorende GUID kunnen mappen voordat we iets kunnen pushen.

**Hoe lossen we dit op?** Twee opties:

1. **Eenmalige migratie (onze voorkeur):** We doen één keer een bulk-opvraging via de API — alle klanten, leveranciers, artikelen — halen de GUIDs op, en slaan die op in SUPWISE. Daarna weten we altijd welke GUID bij welke code hoort.
2. **Per-call lookup:** Bij elke push zoeken we eerst de GUID op. Dat werkt, maar is trager en kost extra API-calls — wat snel in de rate limits loopt bij bulk operaties.

→ *"Wij willen optie 1 — eenmalige sync. Zijn jullie het daarmee eens? En zijn er bezwaren vanuit Exact-kant om alle accounts en artikelen op te halen via de API?"*

---

### Artikelcodes zijn niet uniek

*Dit bepaalt hoe we artikelen mappen bij het pushen van orderregels.*

In SUPWISE kunnen meerdere artikelen dezelfde Exact item code hebben. Hoe zit dat in Exact zelf? Is elke code uniek in Exact, of kunnen meerdere items dezelfde code delen?

- **Als codes in Exact wél uniek zijn** → dan kunnen we op code zoeken en altijd de juiste GUID vinden.
- **Als codes in Exact ook niet uniek zijn** → dan moeten we een andere aanpak bedenken (bijv. op naam + code combinatie zoeken, of handmatig mappen).

---

### Klant én leverancier tegelijk

*In SUPWISE kan een relatie beide rollen hebben. Hoe werkt dat in Exact?*

Een bedrijf in SUPWISE kan zowel klant als leverancier zijn (bijv. een scheepswerf die zowel orders plaatst als materiaal levert). Hoe zit dat in Exact?

- **Eén Account met twee rollen** → dan zoeken we één GUID op en die gebruiken we voor zowel SO als PO.
- **Twee aparte Accounts** → dan moeten we twee GUIDs bijhouden per relatie in SUPWISE.

---

### Push knop voor relaties en artikelen?

*Bepaalt de omvang van wat we bouwen.*

We hadden gepland dat alleen orders een "Push naar Exact" knop krijgen. Maar als klanten, leveranciers of artikelen nog niet in Exact bestaan, moeten we ook die kunnen aanmaken vanuit SUPWISE.

- **Als alles al in Exact staat** → alleen orders pushen, we zijn klaar.
- **Als niet** → dan ook push-functionaliteit voor relaties en/of artikelen — dat is extra scope en moet ingepland worden.

*"Kunnen we nu al beslissen wat de scope is?"*

---

## 5. Vragen over de koppeling (OAuth / API-toegang)

### Wie registreert de OAuth app in het Exact App Centre?

*Iemand met admin-toegang tot het Exact portaal moet dit doen.*

Voor de koppeling moet er een OAuth-applicatie geregistreerd worden in het Exact App Centre. Dat vereist admin-toegang tot jullie Exact-account. Doen jullie dat, of regelen wij dat samen?

---

### Wie beheert de tokens na go-live?

*Dit is een aandachtspunt voor langdurig gebruik.*

De OAuth refresh token verloopt na ~30 dagen inactiviteit. Als niemand een maand lang iets pusht naar Exact, moet iemand opnieuw inloggen om de koppeling te heractiveren. Wie doet dat? En moeten we een alert inbouwen zodat iemand gewaarschuwd wordt vóórdat de token verloopt?

---

### Zijn er al andere API-koppelingen met Exact?

*Als er al integraties zijn, willen we geen conflicten of rate limit-problemen.*

Gebruikt jullie Exact al een koppeling met een ander systeem? Als ja — wat doet die, en hoeveel API-calls maakt die per dag? We willen weten of we de rate limits moeten delen.

---

## 6. Vragen over het proces

### Wanneer wordt een order gepusht naar Exact?

*Dit bepaalt of we een handmatige knop bouwen of een automatische trigger.*

Ons plan was: een medewerker klikt op "Push naar Exact" per order — handmatig, bewust. Is dat wat jullie willen? Of liever automatisch pushen zodra een order een bepaalde status bereikt (bijv. "bevestigd" of "volledig ingekocht")?

- **Handmatig** → meer controle, minder risico op ongewenste pushes.
- **Automatisch** → minder werk, maar we moeten goed afspreken bij welke status.

---

### Wat gebeurt er ná de push in Exact?

*We willen de downstream workflow begrijpen zodat we niets kapot maken.*

Als een verkooporder in Exact staat — wat doet iemand daar dan mee? Worden er facturen aangemaakt vanuit de SO? En de inkooporders — worden die verder verwerkt door de inkoopadministratie? We willen weten hoe de Exact-kant van het proces eruitziet ná onze push.

---

### Drop Shipments?

*Dit verandert onze implementatie als het relevant is.*

Worden goederen soms direct van een leverancier naar het schip (of de klant) gestuurd, zonder via jullie eigen warehouse? Exact heeft hier een speciale DropShipment-koppeling voor die de SO en PO direct aan elkaar linkt.

- **Als drop shipments voorkomen** → dan moeten we dat meenemen in de implementatie.
- **Als niet** → dan kunnen we dit voor nu buiten scope laten.

---

### Backorders — apart pushen of updaten?

*Dit bepaalt of we "update" logica nodig hebben naast "create" logica.*

Als een order deels geleverd wordt — de nalevering (backorder) moet dan ook naar Exact. Hoe willen jullie dat?

- **Als een nieuwe order** → we pushen de backorder als een aparte SO + POs. Simpel.
- **Als een update van de bestaande order** → dan moeten we ook update-functionaliteit bouwen. Dat is complexer.

---

### Wie controleert de sync?

*Toegangsbeheer voor de sync log in SUPWISE.*

Na een push kunnen medewerkers in SUPWISE zien of een order gesynchroniseerd is (of mislukt). Moet de sync log zichtbaar zijn voor alle gebruikers, of alleen voor admins / boekhouding?

---

## 7. Aandachtspunten — proactief benoemen

*Deze punten wil ik zelf aankaarten zodat er geen verrassingen zijn na go-live.*

- **Rate limits:** 60 API-calls per minuut, 5.000 per dag. Een gemiddelde order met 4 leveranciers kost 5 calls. Dat is normaal geen probleem. Maar bij een eenmalige bulk-migratie (alle GUIDs ophalen voor 200k artikelen) moeten we dit zorgvuldig plannen — dat doen we gespreid over meerdere dagen/nachten.

- **Token verloop:** Als niemand 30 dagen lang iets pusht, moet er opnieuw geautoriseerd worden. We kunnen een alert inbouwen die 5 dagen van tevoren waarschuwt. Is dat nuttig?

- **Initiële GUID-migratie:** Voordat we ook maar één order kunnen pushen, moeten we eerst alle GUIDs ophalen. Hoeveel records staan er in Exact (klanten, leveranciers, artikelen)? Dat bepaalt hoe lang die eenmalige initialisatie duurt.

- **Eenrichtingsverkeer in V1:** In de eerste versie pushen we alleen van SUPWISE naar Exact — we halen niks terug. Dat betekent: als iemand in Exact iets aanpast (bijv. een orderregel), ziet SUPWISE dat niet. Is dat acceptabel voor fase 1?

---

## 8. Vervolgstappen — wat we nodig hebben na dit gesprek

- [ ] **Sandbox credentials** — toegang tot een testomgeving
- [ ] **OAuth app registratie** — of hulp daarbij (wie regelt dit?)
- [ ] **Divisie-ID** — welke divisie(s) gebruiken we?
- [ ] **GUIDs ophalen** — ofwel via de API (wij doen het zelf), ofwel een CSV-export vanuit Exact met GUIDs erbij
- [ ] **Beslissing push-scope** — alleen orders, of ook relaties + artikelen?
- [ ] **Contactpersoon** — wie is het technisch aanspreekpunt voor Exact-gerelateerde vragen tijdens development?

---

*Laatste check vóór de meeting: secties 4 (GUID probleem) en 3 (sandbox) zijn de twee punten die alles kunnen blokkeren. Die als eerste aankaarten.*
