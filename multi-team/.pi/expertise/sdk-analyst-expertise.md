# SDK Analyst Expertise

*This file is maintained by the SDK analyst agent. Do not edit manually.*

### Polymarket Rust SDK Structure — 2026-04-06
- **Repo**: `github.com/Polymarket/rs-clob-client`
- **WS code**: `src/ws/` (generic framework) + `src/clob/ws/` (Polymarket-specific)
- **Key files**: `src/clob/ws/types/request.rs` (SubscriptionRequest struct), `src/clob/ws/types/response.rs` (WsMessage enum)
- **Pattern**: SDK uses a generic ConnectionManager + domain-specific message types
- **Subscription**: `{"type":"market","assets_ids":[...],"custom_feature_enabled":true}` — derived from serde Serialize on SubscriptionRequest struct

### Exact Online REST API — 2026-04-07
- **SDKs analyzed**: 8 (Node.js ×2, Python ×2, Go ×1, n8n ×2, API metadata ×1)
- **Auth**: OAuth2 Authorization Code only. No daemon mode. Token endpoint uses `application/x-www-form-urlencoded`, NOT JSON.
- **Token refresh**: Single-use refresh tokens. Must save new refresh_token after every refresh. Mutex needed for concurrent requests.
- **Access token**: ~10 min lifetime. Best practice: refresh proactively at 9:30 mark.
- **CRITICAL — ItemCode is READ-ONLY**: Cannot use `ItemCode` (string) in POST/PUT for SalesOrderLines or PurchaseOrderLines. Must resolve to `Item` (GUID) first.
- **CRITICAL — Account Code padding**: Account `Code` is 18 chars with leading spaces. Filter: `Code eq '           1234567'`. Python SDK uses `'%18s'` format.
- **CRITICAL — PurchaseOrderLine quantity**: Use `QuantityInPurchaseUnits`, NOT `Quantity` (read-only).
- **Mandatory fields**: SalesOrder: `OrderedBy` (GUID) + `SalesOrderLines`. PurchaseOrder: `Supplier` (GUID) + `PurchaseOrderLines`.
- **Pagination**: Follow `d.__next` URL. Default ~60 items/page. Bulk GET endpoints return more.
- **Rate limits**: 100/minute, 9000/day. Headers: `X-RateLimit-Minutely-Remaining`, `X-RateLimit-Remaining`, `X-RateLimit-Minutely-Reset`, `X-RateLimit-Reset`.
- **Best SDK for Node.js reference**: `@quantix-ict/exact-online` — clean TypeScript, proper auth flow.
- **Best API metadata source**: `DannyvdSluijs/ExactOnlineRestApiReference` — machine-readable JSON with per-field POST/PUT/GET flags.
