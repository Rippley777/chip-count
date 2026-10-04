# Chip Count IPC contract

All snake_case. UTC ISO timestamps. Token amounts are integer counts. Money is USD; Rust uses integer nanodollars before final display conversion. `null` means unknown. A command envelope is `{command: string, args: object, demo: boolean}`. Native command is `dispatch(request)`, browser development POST `/api/dispatch` uses the same Rust engine. Demo uses an independent SQLite database populated by Rust deterministic generators; never mock production in JavaScript.

Core API: `pub struct Engine`; `Engine::open(path: &Path) -> anyhow::Result<Self>`; `Engine::demo() -> anyhow::Result<Self>`; `engine.dispatch(command: &str, args: serde_json::Value) -> anyhow::Result<serde_json::Value>`; `engine.reconcile() -> anyhow::Result<()>`; `engine.watch_roots() -> Vec<PathBuf>`. Engine may be held behind Mutex and worked on in spawn_blocking. Caller owns demo selection. Core owns discovery, parser state, migrations, all totals and exports. Native owns dialog, OS reveal, notification, monitoring, tray, window settings.

Commands:
- `snapshot`: `{filter?: Filter}` -> Snapshot. All analytics respect filter; sources/settings/budgets remain global. Sessions cap 500, `total_sessions` preserves count. `top_sessions` ranks the five largest sessions from the full filtered view before pagination (ties use session ID). `reporting` carries a single `as_of` clock instant, resolved half-open UTC and local boundaries, IANA timezone, Monday week start, and comparison boundaries. `Filter.period` accepts `today`, `week`, `month`, `year`, `7`, `30`, `90`, `all`, or `custom`; calendar presets resolve in Rust and end at now. `previous` is matching civil progress in the preceding calendar period, capped at its end; `previous_complete` is the complete preceding calendar period. Rolling/custom comparisons use equal elapsed duration. All-time history has no comparison. Explicit custom date ends include the local date; timestamp ends are exclusive. A custom start without an end resolves through now. Filters matching session may still apply dates to events.
- `session`: `{id, filter?: Filter, event_search?: string, event_offset?: number}` -> Detail. Event search spans the full selected interval; pages contain up to 200 records. `event_count` is the interval count, `event_matches` is the search count, and `event_offset` identifies the page. Search/pagination never change the interval summary.
- `annotate`: `{id, alias?, notes?, tags?: string[], pinned?: boolean}` -> `{ok:true}`
- `source_save`: `{id?, provider:'claude'|'codex', label, path, enabled, exclusions?:string[]}` -> `{ok:true}`
- `source_remove`: `{id}` -> `{ok:true}`
- `rescan`: `{rebuild?:boolean}` -> `{ok:true}`. Rebuild removes derived usage only, preserves sources, annotation, settings; never source files.
- `settings_save`: `{settings: Partial<Settings>}` -> `{ok:true}`
- `budget_save`: `{id?, name, amount, unit:'usd'|'tokens', period:'day'|'month'|'5h'|'window', window_minutes?:number, project?:string, threshold:number}` -> `{ok:true}`
- `budget_remove`: `{id}` -> `{ok:true}`
- `project_save`: `{path, name?, color?, notes?, favorite?, aliases?:string[]}` -> `{ok:true}`
- `pricing_save`: `{model, input, output, cache_read, cache_write}` per-million USD, finite nonnegative -> `{ok:true}`; override affects new observations only
- `reprice`: `{}` -> `{ok:true}`; explicit all-event recalculation with current price versions
- `export`: `{format:'csv'|'json', filter?:Filter, session_ids?:string[], redact_paths:boolean, redact_labels:boolean}` -> `{content, filename, mime}`
- `compare`: `{ids?:string[], ranges?:[{from,to},{from,to}], filter?:Filter, alignment?:'elapsed'|'wall'}` -> `{items: ComparisonItem[]}`

Native-only commands via frontend helpers: pick path (dialog plugin), save content (Rust save dialog and file write command), reveal path (`reveal_path` validating against known source/project paths), `monitoring` `{paused?:boolean}`, `compact` `{}`, `desktop_settings` `{launch_at_login?:boolean, close_to_tray?:boolean, notifications?:boolean}`. Rust emits `index-updated` `{at, paused}`. Monitoring polls reconcile every 3s (metadata/checkpoints only) plus notify watcher. No transcript content stored.

Exact shared response shapes are in src/types.ts. Coordinate extensions there before consuming. Return actionable errors, never transcript bodies.

Budget responses expose `unpriced_tokens` and `unknown_fields`. Known values are lower bounds when coverage is partial; forecasts are suppressed when the relevant token or cost total is incomplete.

Daily buckets include zero-use civil dates with numeric `x` coordinates spaced one day apart. Minute timelines expose epoch milliseconds in `x` (elapsed milliseconds for elapsed comparisons), ordered by actual time including repeated DST minutes. Sparse minute gaps include zero buckets at their edges. JSON exports include `reporting`; CSV appends `reporting_range_json`.
