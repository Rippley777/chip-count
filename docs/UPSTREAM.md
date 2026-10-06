# Source compatibility notes

Reviewed 2026-10-03. Parsers are independently implemented; the sources below were consulted, not vendored. Original synthetic fixtures live in `fixtures/` and Rust tests. No personal transcript is included in this repository.

| Reference | Revision or retrieval | What was checked |
| --- | --- | --- |
| [ccusage](https://github.com/ccusage/ccusage/tree/c294a0ee6b249186bc47491078425c20e8fb045b) | `c294a0ee6b249186bc47491078425c20e8fb045b` | Rust Claude and Codex adapters, identity handling, counters, replay, compaction, fixtures |
| [Codex](https://github.com/openai/codex/tree/6326163b9abd7802c0e57be4e326e5f898bbba75) | `6326163b9abd7802c0e57be4e326e5f898bbba75` | Current upstream revision; representative local rollout schema 0.159.2 |
| [ccusage Claude guide](https://ccusage.com/guide/claude/) | 2026-10-03 | Standard roots and `CLAUDE_CONFIG_DIR` |
| [ccusage Codex guide](https://ccusage.com/guide/codex/) | 2026-10-03 | Active/archive locations and documented provider semantics |
| [Tauri 2](https://v2.tauri.app/) | 2026-10-03 | Native shell, commands, capabilities, packaging |
| [Claude pricing](https://platform.claude.com/docs/en/about-claude/pricing), [model IDs](https://platform.claude.com/docs/en/about-claude/models/model-ids-and-versions), [models overview](https://platform.claude.com/docs/en/models/overview) | 2026-10-06 | Standard rates through Sonnet 5.5, Opus 5.5, Fable/Mythos 5.1; exact IDs, dated snapshots, and cache read/write rates |
| [OpenAI pricing](https://developers.openai.com/api/docs/pricing) | 2026-10-03 | Standard versus fast rates; unlisted model names must remain unpriced |
| [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra), [6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol), [6 Sol](https://developers.openai.com/api/docs/models/gpt-6-sol), [6 Luna](https://developers.openai.com/api/docs/models/gpt-6-luna), [5.6 Sol](https://developers.openai.com/api/docs/models/gpt-5.6-sol), [5.6 Terra](https://developers.openai.com/api/docs/models/gpt-5.6-terra) | 2026-10-03 | Exact model IDs, Standard input/cache/output rates, cache writes at 1.25x input, and the >272K per-request long-context threshold |
| [Codex credit rate card](https://help.openai.com/en/articles/11481834-chatgpt-rate-card-business-enterpriseedu-credit-based-pricing), [USD token rate card](https://help.openai.com/en/articles/20001415-chatgpt-rate-card-enterprise-token-based-pricing), [GPT-5.6 Luna](https://developers.openai.com/api/docs/models/gpt-5.6-luna) | 2026-10-03 | Both Codex rate cards explicitly identify auto review as GPT-5.6 Luna; Standard USD rates are $0.20 input, $0.02 cached input, $1.20 output per million. Codex does not charge for cache writes. |

## Accounting decisions

Claude cache reads and cache writes are additional categories beside its uncached input counter. Codex cached input and reasoning output are subsets of input/output respectively. Chip Count normalizes disjoint input/cache categories and keeps reasoning as an informational subset. A total must never add reasoning twice.

Cumulative counters establish a baseline; a repeated total is not another request. Request identities take priority over token values. Identical usage from separate requests must count twice, while a copied file or a repeated snapshot must not.

Subagents can contain inherited parent records. A relationship alone does not authorize adding a parent's reported rollup. Own usage is canonical; combined usage follows relationships without adding the same event again. Unestablished relationships remain unknown.

A context capacity is not a cumulative usage allowance. Lifetime throughput must never appear as context occupancy. Limits are displayed only from actual provider reports, with timestamps and source scope; a profile name does not establish account identity.

Prices are selected API-equivalent estimates, not billed subscription costs. Preserve an already priced event's recorded pricing version until the user explicitly requests recalculation. When a newly bundled model supplies a previously missing price, startup atomically recovers that model's eligible unpriced events, including dated Claude snapshots. The 2026-10-06 Claude catalog migration also retries supported Claude models already in the catalog once, repairing snapshots missed by the previous exact-model recovery. Local overrides and existing estimates are preserved. Unsupported models or pricing conditions remain visibly incomplete.

ccusage is MIT licensed, copyright ryoppippi and contributors. OpenAI Codex is Apache-2.0 licensed. See upstream projects for their complete notices. Chip Count does not execute either application or require either CLI as a runtime dependency.


## Identity and unsupported data

Claude response identity combines message and request IDs when present, otherwise a message/request/UUID identifier; without strong IDs it falls back to session, timestamp and source line and emits a coverage warning. Codex prefers reported response/request identity, then the session's cumulative snapshot identity. Token values are never a content hash for deduplicating distinct identified requests. Current subagent history ordinals establish which events are inherited; older rewritten histories without a reliable boundary remain explicitly uncertain.

Reported response corrections reconcile in place, including zero corrections and final records with omitted categories. SQLite retains an event's first ingestion timestamp. Copied roots contribute source/profile membership without increasing global totals.

The bundled snapshot covers the explicitly listed models and Standard API-equivalent rates, with verified long-context multipliers for the GPT-6/GPT-5.6 models above. More than 272,000 request input tokens (including cache reads/writes) applies 2x input/cache and 1.5x output rates for that request; output, reasoning, and lifetime throughput do not determine this threshold. Local overrides specify the user's exact category rates without hidden multipliers. Unknown models, unverified fast/service tiers, unsupported long-context conditions and unsupported cache-write durations remain unpriced unless a local override supplies the assumptions. Chip Count does not infer current context from lifetime throughput, infer account-wide quota from a source label, or invent unavailable request durations.

`codex-auto-review` retains its source identifier and uses the current documented GPT-5.6 Luna backend mapping. This is explicitly inferred model pricing because local logs do not establish historical backend routing. Startup recovers previously unpriced review events; priced history and local overrides remain versioned. `Price.inferred`, `Usage.inferred_price_tokens`, event provenance, and CSV/JSON exports preserve this distinction. Costs containing these events display ≈ and explain the auto-review assumption. This is an API-equivalent token-value estimate, not a reconstruction of personal-plan allowances, credit purchases, contractual discounts, or actual invoices.

## Claude catalog recovery — 2026-10-06

The previous catalog omitted Sonnet 5/5.5, Opus 4.7/4.8/5/5.5, Fable 5/5.1, Mythos 5/5.1, and legacy Opus 4/4.1 and Haiku 3.5. These models now have documented Standard API-equivalent category rates. Sonnet 5.5 uses $2 input, $10 output, $0.20 cache reads, and $2.50 five-minute cache writes per million tokens. The allowlist is shared by seeding and dated-snapshot matching, so adding a rate also enables its snapshot recovery without guessing prices for similar model names. Original event model identifiers remain intact.

The migration updates only eligible `unpriced` events and commits the catalog, repaired costs, and migration marker together. It preserves custom rate rows, historical rate versions, and already priced events. Existing unsupported fast-mode and one-hour-cache warnings still prevent automatic pricing; this change does not infer those rates or claim to reproduce provider invoices. Confirmation on the reporting engineer's machine still requires the exact source model and event warning, since no transcript or index from that machine was available during this fix.
