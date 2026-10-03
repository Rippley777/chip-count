# Synthetic usage fixtures

These JSONL files contain metadata only and are authored for Chip Count. They are not user transcripts. They exercise the current Claude response snapshot and Codex cumulative counter shapes consulted in ccusage commit `c294a0ee6b249186bc47491078425c20e8fb045b` and Codex commit `6326163b9abd7802c0e57be4e326e5f898bbba75`.

- `claude/accounting.jsonl`: one response corrected from 10 to 25 output tokens, plus a distinct request with identical usage; total 390 including 100 cache-read and 40 cache-write.
- `codex/accounting.jsonl`: cumulative 100/10 then 150/25 plus repeated snapshot; total 175, including cache-read 60 within input and reasoning 8 within output.

Integration tests generate timestamp-relative files in temporary directories so retention behavior stays reproducible. Parser unit tests include inherited subagent history, counter resets, partial history, response reconciliation and compaction pairing. Demonstration data is generated in `demo.rs` into a separate in-memory database and never substitutes for production data.
