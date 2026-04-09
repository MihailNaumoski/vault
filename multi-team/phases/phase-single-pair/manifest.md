# Task Manifest

**Task:** Single Active Pair Trading with Live Pair Search/Filter UI  
**Created:** 2026-04-08  
**Total Phases:** 2  
**Status:** in-progress  

---

## Phase 1: Design — Single-Pair Trading + Pair Selector

- **Scope:** Architecture and specification for: (1) Engine single-pair mode — only 1 pair active at a time, (2) TUI pair browser screen with live search/filter from websocket data, (3) DB schema for active pair tracking and event logging, (4) Pair selection flow across both markets (Kalshi + Polymarket).
- **Status:** in-progress
- **Dependencies:** none
- **Gate decision:** [pending]

## Phase 2: Build + Validate — Implement and Review

- **Scope:** Rust Engine Dev implements the single-pair mode, TUI pair browser, and DB changes. Code Review + Validation team reviews.
- **Status:** pending
- **Dependencies:** Phase 1 complete
- **Gate decision:** [pending]
