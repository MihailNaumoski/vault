# Development Cost Proposal — Polymarket/Kalshi Arbitrage Bot

**Date:** April 8, 2026

---

## Project Overview

Custom-built arbitrage trading bot that monitors price discrepancies between **Polymarket** and **Kalshi** prediction markets and executes trades to capture profit from the spread.


## Pricing

| Phase | Deliverable | Cost | Payment Trigger |
|-------|------------|------|-----------------|
| **Phase 1 — Scanner** | Cross-platform price scanner that detects arbitrage opportunities and validates whether real profit exists after fees | **€500** | Upfront, before work begins |
| **Phase 2 — Trading Bot** | Full trading bot with automated execution on both Polymarket and Kalshi | **€800** | Upon delivery of finished, trade-executing code |
| **Total** | | **€1,300** | |

---

## Milestone Structure

### Phase 1 — Scanner (€500 upfront)

- Connects to both Polymarket and Kalshi APIs
- Scans for matching markets across platforms
- Calculates real spread after all platform fees
- Produces a clear report on whether arbitrage opportunities exist

**Go/No-Go Gate:** If the scanner demonstrates that fees consume all potential profit, the project stops here. No further payment is owed.

### Phase 2 — Trading Bot (€800 on completion)

- Proceeds only if Phase 1 confirms viable arbitrage
- Automated trade execution on both platforms
- Risk management and position tracking
- Paper trading mode for safe validation before going live
- Automatic unwinding — if one side of a trade fails, the bot reverses the other side to avoid one-sided exposure
- Configurable position size limits to cap risk
- Kill switch to halt all trading instantly
- Full trade logging for auditability
- Payment due when the bot is fully functional and successfully executing trades on both platforms

---

## Hosting & Maintenance

| Service | Cost |
|---------|------|
| Hosting on dedicated Mac Mini server (1 Gbps connection) | **€50/month** |

Includes:
- Server hosting (no need for client to rent a cloud VPS)
- Ongoing maintenance
- Bug fixes
- Uptime monitoring

---

## Summary

| Item | Amount |
|------|--------|
| Development (total) | €1,300 |
| Monthly hosting & maintenance | €50/month |
