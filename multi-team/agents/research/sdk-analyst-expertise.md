# SDK Analyst Expertise

*This file is maintained by the SDK analyst agent. Do not edit manually.*

### Polymarket Rust SDK Structure — 2026-04-06
- **Repo**: `github.com/Polymarket/rs-clob-client`
- **WS code**: `src/ws/` (generic framework) + `src/clob/ws/` (Polymarket-specific)
- **Key files**: `src/clob/ws/types/request.rs` (SubscriptionRequest struct), `src/clob/ws/types/response.rs` (WsMessage enum)
- **Pattern**: SDK uses a generic ConnectionManager + domain-specific message types
- **Subscription**: `{"type":"market","assets_ids":[...],"custom_feature_enabled":true}` — derived from serde Serialize on SubscriptionRequest struct
