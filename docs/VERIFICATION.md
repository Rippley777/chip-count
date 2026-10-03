# Verification

This file records concrete checks rather than treating a browser mock as desktop verification.

## Accounting regression suite

The parser tests exercise:

- Cumulative input/output `100/10`, then `150/25`, produce `150/25`.
- A latest delta and matching cumulative total contribute once.
- Response metadata reconciles with its cumulative record.
- Paired remote compaction is counted; unmatched usage metadata is not invented.
- Local compaction already reflected in counters is not charged again.
- Partial history, including an excerpt with its header, retains carry-in separately.
- Current Codex subagent history ordinals exclude inherited parent usage.
- Cache reads, cache writes and reasoning subsets do not inflate throughput.
- Distinct Claude request IDs with identical usage stay distinct.
- Final Claude snapshots retain identity and establish authority.
- Corrected request snapshots use the original response baseline.
- A counter reset without a reported latest increment cannot invent tokens.
- Provider limit scope is attached to its actual observation.

Additional core integration tests cover SQLite/checkpoint behavior; browser tests exercise the same compiled Rust engine through the development bridge. Native tests validate OS path boundaries and loopback request validation.

## Commands

```sh
npm run test:rust
cargo test -p chip-server
cargo test -p chip-count --lib
npm run typecheck
npm run lint
npm run build
npm test
npm run check:rust
npm run desktop:build
```

Visual artifacts produced by the browser suite are written to the ignored `artifacts/` directory. Laptop target: 1280×800. Desktop target: 1440×960 or larger. The app should be inspected in both themes, with the inspector open, empty/filtered states, and charts.

## Privacy and source boundaries

Representative local log inspection was restricted to record kinds, identifier keys, working-directory metadata, usage counts, and reported limits. Transcript bodies were not printed or copied into fixtures. Production source files are opened read-only; rebuilds affect only Chip Count's derived SQLite tables.

## Packaging

A local macOS bundle is a development artifact until it is signed and notarized with the distributor's Apple identity. No signing identity or credential is bundled. The installed binary embeds Rust/SQLite and the built frontend; Node, Python and the development HTTP service are not runtime dependencies.


## Executed checks — 2026-10-03

- Rust core: 54 tests passed across parsing, checkpoint reconciliation, accounting, analytics, pricing, budgets, source boundaries, and exports.
- Native path validation: 1 test passed. Development API request-boundary validation: 3 tests passed.
- TypeScript type checking, ESLint (zero warnings), production Vite build, formatting, and workspace Clippy with warnings denied passed.
- Browser: all 15 Playwright tests passed against the final Rust backend. Coverage includes canonical Rust totals, combined filters, real JSONL append monitoring, incomplete lines, pause/resume, all eight pages, inspector selection, export download, demo separation, compact mode, event provenance/search, timeline filtering/reset, annotation persistence, draft settings through polling, light theme, custom rolling token budgets, and chart rendering/date drill-down.
- Dark and light layouts were visually inspected at 1280×800 and 1440×960. Live panes scroll independently; the inspector width is adjustable. Recharts entrance animations are disabled so periodic data refreshes do not blank charts.
- A native macOS app was launched and its actual `tauri://localhost` WebView populated from discovered local sources. Main-window pause, compact window creation, and the native save dialog were exercised. The native CSV export produced 415 usage rows with source paths redacted. The final packaged app was then launched with both preview services stopped; embedded indexing, shared main/compact pause-resume state, and the compact Unpriced indicator passed.

Regression refinements include zero-value corrections, authoritative finals with missing categories, source rotation fairness across restart, a cap on all visited directory entries, searchable event pages without changing totals, unknown-pricing budget coverage, and no invented previous-period baseline for all-time reports.

## Pricing recovery — 2026-10-03

- Added official Standard category rates for GPT-6 Astra, GPT-6.1 Sol, GPT-6 Sol, GPT-6 Luna, GPT-5.6 Sol, and GPT-5.6 Terra, including verified request-level long-context pricing.
- All 58 core tests passed. New regressions cover each model's disjoint category costs, reasoning as an output subset, the exact long-context boundary including cached input, local override semantics, unknown model/service tiers, and startup recovery of unpriced history without changing existing priced events or overrides.
- Formatting, workspace Clippy with warnings denied, frontend lint/build, and the macOS desktop production build passed. All 15 browser tests passed with the Rust and Vite services ready; an initial cold-start run hit the development proxy startup race before Rust was listening.
- Relaunched the packaged native app against its existing persistent index. The Live dashboard showed a dollar estimate for today and the previously unpriced Chip Count Astra session showed $82.89. All indexed Astra tokens had price coverage. Unknown `codex-auto-review` usage remained explicitly unpriced.
- A read-only before/after comparison verified all pre-upgrade event identities, token categories, source provenance, and ingestion timestamps. Notes, projects, budgets, source configuration, and settings were preserved. Source read/progress timestamps changed normally during reconciliation.

## Practical boundaries

The macOS Apple Silicon app and ZIP are local development builds; public distribution still needs Developer ID signing and notarization. Windows/Linux release packages were not built in this environment. System notification delivery and login launch depend on the user's OS settings and were not enabled as part of verification.

Unknown model prices and unsupported service/cache tiers stay unpriced. Context occupancy, request duration, explicit completion, and account quota remain unavailable where the logs cannot establish them. Older rewritten subagent logs without a reliable history boundary carry warnings.

Traversal is limited to depth 12, 100,000 visited entries, and 50,000 JSONL files per source, with explicit partial-source diagnostics when capped. Reconciliation uses resumable 1,000-line batches and rotating cursors. Source/event views are paginated; aggregation remains in Rust over indexed metadata and has not been performance-qualified for multi-million-event installations.

## Auto-review pricing — 2026-10-03

- OpenAI's current Codex credit and USD rate cards explicitly identify auto review as GPT-5.6 Luna. Its Standard rates are $0.20 input, $0.02 cached input, and $1.20 output per million tokens. Codex cache writes are not charged. The original `codex-auto-review` model identifier is retained; historical routing is marked as inferred rather than asserted as observed.
- Added `Price.inferred` and additive, backward-compatible `Usage.inferred_price_tokens` metadata. Combined totals, event provenance, and CSV/JSON exports carry the inferred basis. Dashboard, session rows, inspector, project cards, and comparison costs use ≈ when they include auto-review estimates. Settings show the source and allow custom rates.
- All 60 core and 16 browser tests passed. Added regressions cover documented rate arithmetic, cache/reasoning accounting, inferred-token aggregation, custom overrides, startup recovery, export provenance, and dashboard/inspector labels. The updated session-row marker passed the focused browser regression. Frontend lint/build, formatting, workspace Clippy, and native macOS production packaging passed.
- Relaunch against the actual persistent index recovered 429 auto-review usage events at verification time: approximately $0.90, with zero review tokens left unpriced. Previously priced costs and versions, all prior token categories, notes, budgets, and settings were preserved. The native Live dashboard showed the approximation marker and auto-review basis.
- These are documented token-value estimates. Personal-plan allowances, credit purchases, contractual rates, historical routing changes, and actual invoices cannot be reconstructed from local token logs alone.
