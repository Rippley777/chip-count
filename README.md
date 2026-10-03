# Chip Count

**Know where every token went.**

A local-first AI session analytics desktop app for the Rippley Labs ecosystem. Rust owns discovery, indexing, accounting and exports; React renders the same data in the Tauri app and development preview. There is no cloud service, login, API key, or transcript upload.

## Develop

Build requirements: Node.js 20.19+ (22 recommended), npm, Rust 1.95 (pinned in `rust-toolchain.toml`), and [Tauri's platform prerequisites](https://v2.tauri.app/start/prerequisites/). An installed app does not require these tools.

```sh
npm ci
npm run desktop
```

The browser development preview runs both the Rust service and Vite:

```sh
npm run dev
```

Open `http://127.0.0.1:1431`. The loopback service is for development only; the installed app invokes Rust directly. Folder/save dialogs, Finder/Explorer reveal, menu bar, startup integration, and always-on-top windows are desktop features.

```sh
npm run build          # TypeScript + production frontend
npm run lint
npm run test:rust      # independent accounting/index tests
npm test              # browser interaction tests
npm run check:rust     # workspace Clippy
npm run desktop:build # native app bundle
```

## Your data

On launch Chip Count discovers supported local Claude Code and Codex JSONL roots. Add or disable roots in Sources, label profiles, and inspect parser diagnostics. Sources are read-only. Local session notes, aliases, tags, preferences, and budgets belong to Chip Count's SQLite index.

Production starts with actual discovery. **Explore demo** uses a separate in-memory SQLite store with synthetic sessions, goes through the same Rust calculations, and can be exited immediately. Demo data is never inserted into the production index.

`CODEX_HOME` and `CLAUDE_CONFIG_DIR` configure provider discovery. Custom directories and individual JSONL files can be added in Sources. The development server supports `CHIP_COUNT_DATA_DIR` for its own index location; see `cargo run -p chip-server -- --help` for available startup behavior. Do not point it at a source log directory.

## Architecture and correctness

- `crates/chip-core`: independent SQLite index, provider adapters, canonical usage accounting, queries, pricing, budgets, exports, fixtures and tests.
- `crates/chip-server`: loopback development bridge to that core.
- `src-tauri`: native IPC, background filesystem reconciliation, tray and windows, native dialogs and validated OS actions.
- `src`: typed React interface, virtualized session views, charts and inspectors.
- [IPC contract](docs/IPC.md), [source research](docs/UPSTREAM.md), and [verification notes](docs/VERIFICATION.md).

Usage and cost are limited to observed local records. API-equivalent cost is not a bill. Unknown model pricing is shown as unpriced. Provider-reported limits carry their observation timestamp and scope. Inferred activity is based on event gaps and is not human working time. Original model identifiers are retained.

Large histories are ingested in bounded resumable batches, with durable rotating cursors and paginated session/event views. Aggregation runs in Rust over indexed metadata; multi-million-event installations have not been performance-qualified.

No ecosystem telemetry, shared authentication, or cross-application database access is enabled. Chip Count owns its own local analytics; optional future integrations must use explicit versioned contracts.
