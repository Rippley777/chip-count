//! Regression tests through the public Rust API: real JSONL, SQLite, and exports.
use chip_core::Engine;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
use tempfile::TempDir;

struct Fixture {
    // Close SQLite before TempDir removes its files (also required on Windows).
    engine: Engine,
    db: std::path::PathBuf,
    logs: std::path::PathBuf,
    _temp: TempDir,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("index.sqlite");
        let logs = temp.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let engine = Engine::isolated(&db).unwrap();
        Self {
            _temp: temp,
            db,
            logs,
            engine,
        }
    }
    fn add_source(&mut self, provider: &str, path: &Path, label: &str) {
        self.engine
            .dispatch(
                "source_save",
                json!({"provider":provider,"path":path,"label":label,"enabled":true}),
            )
            .unwrap();
    }
    fn scan(&mut self, provider: &str) {
        self.add_source(provider, &self.logs.clone(), "Personal");
    }
    fn snapshot(&mut self) -> Value {
        self.engine.dispatch("snapshot", json!({})).unwrap()
    }
    fn total(&mut self) -> u64 {
        self.snapshot()["totals"]["total"].as_u64().unwrap()
    }
    fn reopen(&mut self) {
        self.engine = Engine::isolated(&self.db).unwrap();
    }
}
fn at(seconds: i64) -> String {
    (Utc::now() - Duration::hours(1) + Duration::seconds(seconds)).to_rfc3339()
}
fn claude(session: &str, request: &str, input: u64, output: u64, seconds: i64) -> Value {
    json!({"type":"assistant","timestamp":at(seconds),"sessionId":session,"requestId":request,"uuid":format!("uuid-{request}"),"cwd":"/workspace/private_repo","message":{"id":format!("msg-{request}"),"role":"assistant","model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":input,"output_tokens":output,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"text","text":"PRIVATE_TRANSCRIPT_BODY"}]}})
}
fn codex_count(input: u64, output: u64, seconds: i64) -> Value {
    json!({"type":"event_msg","timestamp":at(seconds),"payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":input,"output_tokens":output,"cached_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":input+output},"model_context_window":200000}}})
}
fn write(path: &Path, records: &[Value]) {
    fs::write(
        path,
        records.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
}
fn append(path: &Path, value: &Value) {
    writeln!(
        fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{value}"
    )
    .unwrap();
}

#[test]
fn codex_counters_restart_reimport_and_archive_overlap_preserve_totals() {
    let mut f = Fixture::new();
    let log = f.logs.join("session.jsonl");
    write(
        &log,
        &[
            json!({"type":"session_meta","timestamp":at(0),"payload":{"id":"thread-a","cwd":"/work/app"}}),
            json!({"type":"turn_context","timestamp":at(0),"payload":{"model":"gpt-5.3-codex"}}),
            codex_count(100, 10, 1),
            codex_count(150, 25, 2),
        ],
    );
    f.scan("codex");
    assert_eq!(f.total(), 175);
    assert_eq!(f.snapshot()["totals"]["input"], 150);
    f.reopen();
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 175);
    f.engine.dispatch("rescan", json!({})).unwrap();
    assert_eq!(f.total(), 175);
    let archive = f.logs.join("archive");
    fs::create_dir(&archive).unwrap();
    fs::copy(&log, archive.join("copied.jsonl")).unwrap();
    f.add_source("codex", &archive, "Archive");
    assert_eq!(
        f.total(),
        175,
        "A nested source and archived copy must not multiply counters"
    );
    f.engine
        .dispatch("rescan", json!({"rebuild":true}))
        .unwrap();
    assert_eq!(f.total(), 175);
    assert!(log.exists(), "Rebuilding must leave source logs intact");
}

#[test]
fn partial_trailing_lines_resume_after_restart_without_loss_or_duplication() {
    let mut f = Fixture::new();
    let log = f.logs.join("live.jsonl");
    let first = claude("live", "request-1", 100, 10, 0);
    let second = claude("live", "request-2", 50, 5, 1).to_string();
    fs::write(&log, format!("{first}\n{}", &second[..second.len() / 2])).unwrap();
    f.scan("claude");
    assert_eq!(f.total(), 110);
    f.reopen();
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 110);
    let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
    writeln!(file, "{}", &second[second.len() / 2..]).unwrap();
    drop(file);
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 165);
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 165);
    assert_eq!(f.snapshot()["totals"]["events"], 2);
}

#[test]
fn corrected_snapshot_reconciles_but_distinct_equal_requests_both_count() {
    let mut f = Fixture::new();
    let log = f.logs.join("responses.jsonl");
    let mut intermediate = claude("live", "request-1", 100, 10, 0);
    intermediate["message"]["stop_reason"] = Value::Null;
    write(&log, &[intermediate]);
    f.scan("claude");
    assert_eq!(f.total(), 110);
    append(&log, &claude("live", "request-1", 150, 25, 1));
    append(&log, &claude("live", "request-2", 150, 25, 2));
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 350);
    assert_eq!(f.snapshot()["totals"]["events"], 2);
    f.reopen();
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 350);
}

#[test]
fn truncated_and_rotated_file_replaces_only_its_obsolete_usage() {
    let mut f = Fixture::new();
    let log = f.logs.join("rotate.jsonl");
    write(
        &log,
        &[
            claude("first", "old-a", 100, 10, 0),
            claude("first", "old-b", 50, 5, 1),
        ],
    );
    f.scan("claude");
    assert_eq!(f.total(), 165);
    write(&log, &[claude("second", "replacement", 20, 2, 2)]);
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 22);
    let replacement = f.logs.join("new-file.tmp");
    write(&replacement, &[claude("third", "rotated", 40, 4, 3)]);
    fs::rename(replacement, &log).unwrap();
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 44);
}

#[test]
fn malformed_lines_are_skipped_and_transcript_bodies_are_not_persisted() {
    let mut f = Fixture::new();
    let log = f.logs.join("malformed.jsonl");
    fs::write(
        &log,
        format!(
            "{}\nTHIS IS NOT JSON\n{}\n",
            claude("private", "a", 10, 1, 0),
            claude("private", "b", 20, 2, 1)
        ),
    )
    .unwrap();
    f.scan("claude");
    assert_eq!(f.total(), 33);
    assert!(f.snapshot()["sources"][0]["warnings"].as_u64().unwrap() > 0);
    let detail = f
        .engine
        .dispatch("session", json!({"id":"claude:private"}))
        .unwrap();
    assert!(!detail.to_string().contains("PRIVATE_TRANSCRIPT_BODY"));
    let db = rusqlite::Connection::open(&f.db).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM events WHERE data LIKE '%PRIVATE_TRANSCRIPT_BODY%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn parent_own_and_child_usage_count_once_when_rollup_is_present() {
    let mut f = Fixture::new();
    let own = claude("parent", "parent-request", 100, 0, 0);
    let mut rollup = claude("parent", "rollup-request", 140, 0, 2);
    rollup["usageScope"] = json!("combined");
    let mut child = claude("parent", "child-request", 40, 0, 1);
    child["agentId"] = json!("worker");
    write(&f.logs.join("parent.jsonl"), &[own, rollup]);
    write(&f.logs.join("agent-worker.jsonl"), &[child]);
    f.scan("claude");
    let snapshot = f.snapshot();
    assert_eq!(snapshot["totals"]["total"], 140);
    let parent = snapshot["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "claude:parent")
        .unwrap();
    assert_eq!(parent["usage"]["total"], 100);
    assert_eq!(parent["combined_usage"]["total"], 140);
    assert_eq!(parent["subagents"], 1);
    let detail = f
        .engine
        .dispatch("session", json!({"id":"claude:parent"}))
        .unwrap();
    assert_eq!(detail["session"]["usage"]["total"], 100);
    let exported = f
        .engine
        .dispatch(
            "export",
            json!({"format":"json","redact_paths":true,"redact_labels":true}),
        )
        .unwrap();
    let data: Value = serde_json::from_str(exported["content"].as_str().unwrap()).unwrap();
    let exported_sum: u64 = data["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["usage"]["total"].as_u64().unwrap())
        .sum();
    assert_eq!(
        exported_sum, 140,
        "Exported event rows must sum to the same total as the dashboard"
    );
}

#[test]
fn disabling_source_excludes_its_usage_and_reenable_restores_without_duplication() {
    let mut f = Fixture::new();
    write(&f.logs.join("a.jsonl"), &[claude("a", "a", 100, 10, 0)]);
    f.scan("claude");
    assert_eq!(f.total(), 110);
    let id = f.snapshot()["sources"][0]["id"].clone();
    f.engine
        .dispatch(
            "source_save",
            json!({"id":id,"provider":"claude","path":f.logs,"label":"Work","enabled":false}),
        )
        .unwrap();
    assert_eq!(f.total(), 0);
    f.engine
        .dispatch(
            "source_save",
            json!({"id":id,"provider":"claude","path":f.logs,"label":"Work","enabled":true}),
        )
        .unwrap();
    assert_eq!(f.total(), 110);
}

#[test]
fn price_overrides_preserve_observed_estimates_until_explicit_reprice() {
    let mut f = Fixture::new();
    let log = f.logs.join("prices.jsonl");
    write(&log, &[claude("price", "old", 100, 0, 0)]);
    f.scan("claude");
    let initial = f.snapshot()["totals"]["cost"].as_f64().unwrap();
    assert!((initial - 0.0003).abs() < 1e-10);
    f.engine.dispatch("pricing_save",json!({"model":"claude-sonnet-4-6","input":10,"output":30,"cache_read":1,"cache_write":12})).unwrap();
    f.engine.dispatch("rescan", json!({})).unwrap();
    assert_eq!(f.snapshot()["totals"]["cost"].as_f64().unwrap(), initial);
    append(&log, &claude("price", "new", 100, 0, 1));
    f.engine.reconcile().unwrap();
    assert!((f.snapshot()["totals"]["cost"].as_f64().unwrap() - 0.0013).abs() < 1e-10);
    f.reopen();
    assert!((f.snapshot()["totals"]["cost"].as_f64().unwrap() - 0.0013).abs() < 1e-10);
    let exported = f
        .engine
        .dispatch(
            "export",
            json!({"format":"json","redact_paths":false,"redact_labels":false}),
        )
        .unwrap();
    let data: Value = serde_json::from_str(exported["content"].as_str().unwrap()).unwrap();
    let history = data["pricing_history"].as_array().unwrap();
    for event in data["events"].as_array().unwrap() {
        let price = history
            .iter()
            .find(|p| p["model"] == event["model"] && p["version"] == event["pricing_version"])
            .unwrap();
        let reproduced = event["usage"]["input"].as_f64().unwrap()
            * price["input"].as_f64().unwrap()
            / 1_000_000.0;
        assert!((reproduced - event["usage"]["cost"].as_f64().unwrap()).abs() < 1e-10);
    }
    f.engine.dispatch("reprice", json!({})).unwrap();
    assert!((f.snapshot()["totals"]["cost"].as_f64().unwrap() - 0.002).abs() < 1e-10);
}

#[test]
fn new_price_catalog_recovers_existing_unpriced_sessions_without_rewriting_priced_history() {
    let mut f = Fixture::new();
    let log = f.logs.join("missing-prices.jsonl");
    write(
        &log,
        &[
            json!({"type":"session_meta","timestamp":at(0),"payload":{"id":"price-recovery","cwd":"/work/app"}}),
            json!({"type":"turn_context","timestamp":at(0),"payload":{"model":"gpt-6-astra"}}),
            codex_count(100, 10, 1),
            json!({"type":"turn_context","timestamp":at(2),"payload":{"model":"gpt-5.3-codex"}}),
            codex_count(200, 20, 3),
            json!({"type":"turn_context","timestamp":at(4),"payload":{"model":"unlisted-review-model"}}),
            codex_count(300, 30, 5),
        ],
    );
    f.scan("codex");
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    let priced_before: String = conn
        .query_row(
            "SELECT data FROM events WHERE model='gpt-5.3-codex'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // Recreate an older installation: Astra was absent from the catalog and pinned as unpriced.
    conn.execute("DELETE FROM prices WHERE id='gpt-6-astra'", [])
        .unwrap();
    conn.execute("UPDATE events SET nano=0,data=json_set(data,'$.pricing_version','unpriced','$.usage.cost',0,'$.usage.unpriced_tokens',json_extract(data,'$.usage.total')) WHERE model='gpt-6-astra'", []).unwrap();
    f.engine
        .dispatch(
            "pricing_save",
            json!({"model":"gpt-6-sol","input":99,"output":1,"cache_read":1,"cache_write":1}),
        )
        .unwrap();
    let override_before: String = conn
        .query_row("SELECT data FROM prices WHERE id='gpt-6-sol'", [], |r| {
            r.get(0)
        })
        .unwrap();
    f.reopen();
    assert_eq!(f.total(), 330);
    assert_eq!(f.snapshot()["totals"]["unpriced_tokens"], 110);
    let (version, nano): (String, i64) = conn.query_row("SELECT json_extract(data,'$.pricing_version'),nano FROM events WHERE model='gpt-6-astra'", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(version, "bundled-2026-10-03-openai-v2");
    assert_eq!(nano, 1_500_000);
    let priced_after: String = conn
        .query_row(
            "SELECT data FROM events WHERE model='gpt-5.3-codex'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(priced_before, priced_after);
    let override_after: String = conn
        .query_row("SELECT data FROM prices WHERE id='gpt-6-sol'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(override_before, override_after);
    f.reopen();
    f.engine.reconcile().unwrap();
    assert_eq!(f.total(), 330);
    assert_eq!(f.snapshot()["totals"]["unpriced_tokens"], 110);
}

#[test]
fn auto_review_history_recovers_as_an_inferred_estimate_and_exports_its_basis() {
    let mut f = Fixture::new();
    let log = f.logs.join("auto-review.jsonl");
    let mut count = codex_count(1500, 250, 1);
    count["payload"]["info"]["total_token_usage"]["cached_input_tokens"] = json!(1000);
    write(
        &log,
        &[
            json!({"type":"session_meta","timestamp":at(0),"payload":{"id":"review-recovery","cwd":"/work/app"}}),
            json!({"type":"turn_context","timestamp":at(0),"payload":{"model":"codex-auto-review"}}),
            count,
        ],
    );
    f.scan("codex");
    let conn = rusqlite::Connection::open(&f.db).unwrap();
    conn.execute("DELETE FROM prices WHERE id='codex-auto-review'", [])
        .unwrap();
    conn.execute("UPDATE events SET nano=0,data=json_set(data,'$.pricing_version','unpriced','$.usage.cost',0,'$.usage.inferred_price_tokens',0,'$.usage.unpriced_tokens',json_extract(data,'$.usage.total'))", []).unwrap();
    f.reopen();
    let totals = f.snapshot()["totals"].clone();
    assert_eq!(totals["total"], 1750);
    assert_eq!(totals["unpriced_tokens"], 0);
    assert_eq!(totals["inferred_price_tokens"], 1750);
    assert!((totals["cost"].as_f64().unwrap() - 0.00042).abs() < 1e-10);
    let exported = f
        .engine
        .dispatch("export", json!({"format":"json"}))
        .unwrap();
    let exported: Value = serde_json::from_str(exported["content"].as_str().unwrap()).unwrap();
    let event = &exported["events"][0];
    assert_eq!(event["model"], "codex-auto-review");
    assert!(event["warnings"].as_array().unwrap().iter().any(|w| w
        .as_str()
        .unwrap()
        .contains("model inferred as GPT-5.6 Luna")));
    assert!(exported["pricing_history"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["model"] == "codex-auto-review" && p["inferred"] == true));
    let csv = f
        .engine
        .dispatch("export", json!({"format":"csv"}))
        .unwrap();
    assert!(csv["content"]
        .as_str()
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .ends_with("inferred_price_tokens,reporting_range_json"));
    f.reopen();
    f.engine.reconcile().unwrap();
    assert_eq!(f.snapshot()["totals"]["inferred_price_tokens"], 1750);
}

#[test]
fn exports_apply_filters_redact_paths_labels_and_escape_csv_formulas() {
    let mut f = Fixture::new();
    let mut malicious = claude("export", "formula", 100, 10, 0);
    malicious["message"]["model"] = json!("=1+2");
    write(&f.logs.join("export.jsonl"), &[malicious]);
    write(
        &f.logs.join("other.jsonl"),
        &[claude("other", "not-exported", 200, 20, 1)],
    );
    f.scan("claude");
    f.engine.dispatch("annotate",json!({"id":"claude:export","alias":"Secret customer","notes":"PRIVATE_NOTES","tags":["client-secret"]})).unwrap();
    let args = json!({"format":"json","session_ids":["claude:export"],"redact_paths":true,"redact_labels":true});
    let exported = f.engine.dispatch("export", args).unwrap();
    let content = exported["content"].as_str().unwrap();
    let data: Value = serde_json::from_str(content).unwrap();
    assert_eq!(data["totals"]["total"], 110);
    assert_eq!(data["totals"]["unpriced_tokens"], 110);
    assert_eq!(data["sessions"].as_array().unwrap().len(), 1);
    assert!(!content.contains("/workspace/private_repo"));
    assert!(!content.contains(f.logs.to_str().unwrap()));
    for sensitive in [
        "Secret customer",
        "PRIVATE_NOTES",
        "client-secret",
        "PRIVATE_TRANSCRIPT_BODY",
    ] {
        assert!(!content.contains(sensitive));
    }
    assert!(data["pricing"].is_array());
    assert!(data["units"].is_object());
    assert!(data["timezone"].is_string());
    let csv=f.engine.dispatch("export",json!({"format":"csv","session_ids":["claude:export"],"redact_paths":true,"redact_labels":true})).unwrap();
    let csv = csv["content"].as_str().unwrap();
    assert!(csv.contains("\"'=1+2\""));
    assert!(!csv.contains(f.logs.to_str().unwrap()));
    assert!(csv.contains("known_cost_usd"));
}

#[test]
fn multi_session_jsonl_import_retains_each_session_metadata() {
    let mut f = Fixture::new();
    write(
        &f.logs.join("combined-import.jsonl"),
        &[
            claude("first-session", "first-request", 100, 10, 0),
            claude("second-session", "second-request", 50, 5, 1),
        ],
    );
    f.scan("claude");
    let snapshot = f.snapshot();
    assert_eq!(snapshot["totals"]["total"], 165);
    assert_eq!(snapshot["total_sessions"], 2);
    for id in ["claude:first-session", "claude:second-session"] {
        let detail = f.engine.dispatch("session", json!({"id": id})).unwrap();
        assert_eq!(detail["session"]["id"], id);
        f.engine
            .dispatch("annotate", json!({"id":id,"alias":"Imported session"}))
            .unwrap();
    }
}
