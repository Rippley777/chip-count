# Source compatibility notes

Reviewed 2026-10-03. Parsers are independently implemented; the sources below were consulted, not vendored. Original synthetic fixtures live in `fixtures/` and Rust tests. No personal transcript is included in this repository.

| Reference | Revision or retrieval | What was checked |
| --- | --- | --- |
| [ccusage](https://github.com/ccusage/ccusage/tree/c294a0ee6b249186bc47491078425c20e8fb045b) | `c294a0ee6b249186bc47491078425c20e8fb045b` | Rust Claude and Codex adapters, identity handling, counters, replay, compaction, fixtures |
| [Codex](https://github.com/openai/codex/tree/6326163b9abd7802c0e57be4e326e5f898bbba75) | `6326163b9abd7802c0e57be4e326e5f898bbba75` | Current upstream revision; representative local rollout schema 0.159.2 |
| [ccusage Claude guide](https://ccusage.com/guide/claude/) | 2026-10-03 | Standard roots and `CLAUDE_CONFIG_DIR` |
| [ccusage Codex guide](https://ccusage.com/guide/codex/) | 2026-10-03 | Active/archive locations and documented provider semantics |
| [Tauri 2](https://v2.tauri.app/) | 2026-10-03 | Native shell, commands, capabilities, packaging |
| [Claude pricing](https://platform.claude.com/docs/en/about-claude/pricing) | 2026-10-03 | Standard token rates and cache write duration differences |
| [OpenAI pricing](https://developers.openai.com/api/docs/pricing) | 2026-10-03 | Standard versus fast rates; unlisted model names must remain unpriced |
| [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra), [6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol), [6 Sol](https://developers.openai.com/api/docs/models/gpt-6-sol), [6 Luna](https://developers.openai.com/api/docs/models/gpt-6-luna), [5.6 Sol](https://developers.openai.com/api/docs/models/gpt-5.6-sol), [5.6 Terra](https://developers.openai.com/api/docs/models/gpt-5.6-terra) | 2026-10-03 | Exact model IDs, Standard input/cache/output rates, cache writes at 1.25x input, and the >272K per-request long-context threshold |

## Accounting decisions

Claude cache reads and cache writes are additional categories beside its uncached input counter. Codex cached input and reasoning output are subsets of input/output respectively. Chip Count normalizes disjoint input/cache categories and keeps reasoning as an informational subset. A total must never add reasoning twice.

Cumulative counters establish a baseline; a repeated total is not another request. Request identities take priority over token values. Identical usage from separate requests must count twice, while a copied file or a repeated snapshot must not.

Subagents can contain inherited parent records. A relationship alone does not authorize adding a parent's reported rollup. Own usage is canonical; combined usage follows relationships without adding the same event again. Unestablished relationships remain unknown.

A context capacity is not a cumulative usage allowance. Lifetime throughput must never appear as context occupancy. Limits are displayed only from actual provider reports, with timestamps and source scope; a profile name does not establish account identity.

Prices are selected API-equivalent estimates, not billed subscription costs. Preserve an already priced event's recorded pricing version until the user explicitly requests recalculation. When a newly bundled model supplies a previously missing price, startup atomically recovers that model's eligible unpriced events. Local overrides and existing estimates are preserved. Unsupported models or pricing conditions remain visibly incomplete.

ccusage is MIT licensed, copyright ryoppippi and contributors. OpenAI Codex is Apache-2.0 licensed. See upstream projects for their complete notices. Chip Count does not execute either application or require either CLI as a runtime dependency.


## Identity and unsupported data

Claude response identity combines message and request IDs when present, otherwise a message/request/UUID identifier; without strong IDs it falls back to session, timestamp and source line and emits a coverage warning. Codex prefers reported response/request identity, then the session's cumulative snapshot identity. Token values are never a content hash for deduplicating distinct identified requests. Current subagent history ordinals establish which events are inherited; older rewritten histories without a reliable boundary remain explicitly uncertain.

Reported response corrections reconcile in place, including zero corrections and final records with omitted categories. SQLite retains an event's first ingestion timestamp. Copied roots contribute source/profile membership without increasing global totals.

The bundled snapshot covers the explicitly listed models and Standard rates, with verified long-context multipliers for the six GPT-6/GPT-5.6 models above. More than 272,000 request input tokens (including cache reads/writes) applies 2x input/cache and 1.5x output rates for that request; output, reasoning, and lifetime throughput do not determine this threshold. Local overrides specify the user's exact category rates without hidden multipliers. Unknown models (including `codex-auto-review`), unverified fast/service tiers, unsupported long-context conditions and unsupported cache-write durations remain unpriced unless a local override supplies the assumptions. Chip Count does not infer current context from lifetime throughput, infer account-wide quota from a source label, or invent unavailable request durations.
