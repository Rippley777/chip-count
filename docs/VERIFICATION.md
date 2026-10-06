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

## macOS release automation — 2026-10-03

- Added signing/build, packaging, Keychain credential setup, notarization, stapling, Gatekeeper verification, diagnostic log, and complete native/universal release scripts. A separate Tauri release config explicitly enables hardened runtime.
- Five workflow regressions passed using mocked Apple tools: accepted submission ordering and ZIP recreation, Invalid status despite a successful tool exit, rejection of ad-hoc signing before upload, rejection of disabled Gatekeeper as verification, and missing identity failure before building. Subprocess arguments preserve spaces and avoid shell evaluation. Secrets are not embedded in scripts.
- Native preflight located Xcode, `notarytool`, and `stapler`. An unrestricted Keychain check confirmed a valid Developer ID Application identity; the sandboxed check could not see those identities. The existing development bundle was rejected by signature verification before packaging or any upload, as required. Real signing/submission was not performed: the guide supplies the certificate environment setup and interactive Keychain profile step for the user to run.
- Script syntax, release config compatibility with the installed Tauri schema, formatting, frontend lint/type checking/production build, and whitespace checks passed. No Rust runtime behavior changed.

## Calendar reporting and chart corrections — 2026-10-04

- Rust resolves calendar periods in the saved IANA timezone, using a single `as_of` clock instant. New workspaces detect the OS timezone; saved overrides persist. Weeks start Monday. Current calendar periods compare matching civil progress to date, with complete prior totals reported separately. Custom/rolling filters and CSV/JSON exports share the same half-open event boundaries.
- All 69 core tests passed, including exact midnight and day/week/month/year rollover, leap day, Chicago spring/fall DST, Kathmandu's non-US fractional offset, timezone override persistence, sessions spanning midnight, cross-surface/export agreement, sparse zero-day and elapsed-minute spacing, category totals, and global ranking before pagination.
- All 17 browser tests passed. Calendar preset selection survives navigation and uses Rust metadata; numeric-axis tooltip/day drilling works; cost legends and partial/inferred disclosures are visible. Existing inspector, polling, annotation, export, monitoring, and theme regressions remain green. Browser startup now waits for the proxied Rust health endpoint as well as Vite.
- Frontend production build/lint, workspace Clippy with warnings denied, formatting, and whitespace checks passed. Dark calendar-reporting and existing light analytics previews were inspected. The misleading activity calendar and Live rate sparkline were removed; retained session sparklines disclose their last-observation-hour timescale. No new charts were added.
- The original 2,000-session / 20,000-event audit probe returned `claude:audit-0` first in `top_sessions` with 10,000,100 tokens, while the highest session in the default 500-row page had only 1,100 tokens. Daily buckets covered all 31 calendar days and export totals agreed. Probe snapshot time was approximately 1.36 seconds on this machine; this is a local fixture measurement, not a large-installation performance guarantee.

## Recoverable errors and keyboard flows — 2026-10-04

- All 83 Rust tests and all 26 Playwright tests passed. New coverage exercises corrupt/unavailable index startup, preservation and validated restore, rejected backups, disconnected-source rebuild refusal, source reselection with partial history, malformed JSONL file/line diagnostics, invalid timezone/rates, failed writes, and retention of notes, projects, prices, budgets and historical pricing through rebuild/upgrade.
- Browser keyboard checks use Tab, Enter, shortcuts and row navigation to configure a real source, inspect source diagnostics, traverse virtualized session history, filter history, change settings, select chart dates, page results, open Help and export. They assert dialog focus return, retained drafts and persistent validation messages.
- Native IPC tests run accounting against the Rust bridge and inject only native panels/events for deterministic source/export/restore cancellation, export write failure, startup error payloads, and Settings/Help routing. These are boundary tests, not assertions of visually verified Cocoa behavior.
- TypeScript checks, ESLint without warnings, workspace Clippy with warnings denied, formatting, whitespace checks, production Vite build and macOS desktop app packaging passed. The App Store feature configuration compiled successfully.
- An isolated corrupt-index native smoke process was launched and remained running; its original test database bytes were unchanged. Visual inspection of the native dialog/menu was unavailable because the Mac was locked. The isolated test process was stopped. Actual Cocoa panel interaction and VoiceOver behavior therefore remain unverified in this session.
- Rebuild re-reads checkpoints without discarding cached observations. Reselecting a changed source likewise retains history, including observations whose logs are absent from the replacement root, and exposes the preservation snapshot path in source diagnostics. Recovery copies and upgrade snapshots are retained locally; they are not automatically removed.

## Claude pricing catalog — 2026-10-06

- Reproduced missing coverage using synthetic Claude Code JSONL for `claude-sonnet-5-5`. At 1,000 input, 2,000 output, 3,000 cache-read and 4,000 cache-write tokens, the API-equivalent estimate is $0.0326; totals and JSON exports agree across restart.
- Expanded the verified Claude catalog and shared its model allowlist with dated-snapshot lookup. Tests also reject unknown versions and malformed snapshot suffixes.
- Simulated an older SQLite index with unpriced current and dated Claude models. Startup repairs eligible events, including aliases whose family rate already existed. Already priced history and local overrides remain unchanged; unknown models, fast mode and one-hour cache writes stay explicitly unpriced. A second restart neither doubles usage nor changes the repaired total.
- All 80 core tests passed; workspace Clippy, frontend lint, formatting, and the TypeScript/production frontend build passed. `npm run desktop:build` also produced `target/release/bundle/macos/Chip Count.app`; this is a local build, not a newly notarized release or an installation on the reporting machine.
- These are local fixtures and migration tests, not verification of the reporting engineer's installation. The original report established visible tokens and unpriced costs; the precise model was described as probably the latest Sonnet.

## Chip Count 0.1.1 macOS release — 2026-10-06

- Built the current application as Apple Silicon v0.1.1, including the pending index-recovery improvements and the Claude catalog fix. All 26 Playwright checks and eight release/App Store workflow checks passed; the core pricing fix previously passed all 80 core tests, lint, type checks and workspace Clippy.
- Signed with `Developer ID Application: Ally Rippley (YM4H8YJWGU)`, Hardened Runtime and a secure timestamp. Apple accepted the app ZIP submission `57ad8354-a6b6-44dc-8e97-f53831e33d6d`. The app ticket was stapled and Gatekeeper reported `Notarized Developer ID`; the final ZIP was rebuilt from the stapled app.
- Created the DMG from the stapled app, signed it independently and received Apple acceptance for submission `4f993854-1c1c-45f3-9b45-d45a6f27364b`. The DMG ticket, integrity, and Gatekeeper approval were verified. Its mounted app also passed signature, stapler and Gatekeeper validation.
- Local verified artifacts and the checksum manifest are retained under ignored `artifacts/release-0.1.1/`. The website importer independently verifies the DMG and the app extracted from its generated ZIP before publishing either package. No personal source logs or Apple credentials are included.
