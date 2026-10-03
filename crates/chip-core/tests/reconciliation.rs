//! Authority, provenance, and freshness regressions through durable public ingestion.
use chip_core::Engine;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
use tempfile::TempDir;

struct TestIndex {
    _temp: TempDir,
    engine: Engine,
    root: std::path::PathBuf,
}
impl TestIndex {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("logs");
        fs::create_dir(&root).unwrap();
        let engine = Engine::isolated(&temp.path().join("index.sqlite")).unwrap();
        Self {
            _temp: temp,
            engine,
            root,
        }
    }
    fn import(&mut self, path: &Path, id: &str) {
        self.engine
            .dispatch(
                "source_save",
                json!({"id":id,"provider":"claude","path":path,"label":id,"enabled":true}),
            )
            .unwrap();
    }
    fn snapshot(&mut self) -> Value {
        self.engine.dispatch("snapshot", json!({})).unwrap()
    }
    fn detail(&mut self, id: &str) -> Value {
        self.engine.dispatch("session", json!({"id":id})).unwrap()
    }
}
fn response(input: u64, output: u64, seconds: i64, final_record: bool) -> Value {
    json!({"type":"assistant","sessionId":"parent","requestId":"same-request","uuid":format!("observation-{seconds}"),"cwd":"/repo","timestamp":(Utc::now()-Duration::minutes(5)+Duration::seconds(seconds)).to_rfc3339(),"message":{"id":"same-message","role":"assistant","model":"claude-sonnet-4-6","stop_reason":if final_record{Some("end_turn")}else{None},"usage":{"input_tokens":input,"output_tokens":output,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}})
}
fn write(path: &Path, records: &[Value]) {
    fs::write(
        path,
        records.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
}
fn append(path: &Path, record: &Value) {
    writeln!(
        fs::OpenOptions::new().append(true).open(path).unwrap(),
        "{record}"
    )
    .unwrap();
}

#[test]
fn authoritative_final_beats_later_intermediate_and_can_correct_downward() {
    let mut t = TestIndex::new();
    let path = t.root.join("responses.jsonl");
    write(&path, &[response(100, 25, 0, true)]);
    t.import(&path, "primary");
    append(&path, &response(999, 999, 1, false));
    t.engine.reconcile().unwrap();
    let after_intermediate = t.snapshot();
    assert_eq!(after_intermediate["totals"]["total"], 125);
    assert_eq!(after_intermediate["totals"]["events"], 1);
    assert_eq!(
        t.detail("claude:parent")["events"][0]["kind"],
        "response-final"
    );
    append(&path, &response(80, 15, 2, true));
    t.engine.reconcile().unwrap();
    assert_eq!(
        t.snapshot()["totals"]["total"],
        95,
        "A later authoritative correction may reduce usage"
    );
}

#[test]
fn missing_final_categories_keep_previously_reported_values_and_clear_unknowns() {
    let mut t = TestIndex::new();
    let path = t.root.join("partial-final.jsonl");
    let mut intermediate = response(100, 10, 0, false);
    intermediate["message"]["usage"]["cache_read_input_tokens"] = json!(50);
    write(&path, &[intermediate]);
    t.import(&path, "primary");
    let mut final_record = response(0, 25, 1, true);
    final_record["message"]["usage"]
        .as_object_mut()
        .unwrap()
        .remove("input_tokens");
    final_record["message"]["usage"]
        .as_object_mut()
        .unwrap()
        .remove("cache_read_input_tokens");
    append(&path, &final_record);
    t.engine.reconcile().unwrap();
    let detail = t.detail("claude:parent");
    let usage = &detail["events"][0]["usage"];
    assert_eq!(usage["input"], 100);
    assert_eq!(usage["cache_read"], 50);
    assert_eq!(usage["output"], 25);
    assert_eq!(usage["total"], 175);
    assert!(!usage["unknown_fields"]
        .as_array()
        .unwrap()
        .contains(&json!("input")));
    assert!(!usage["unknown_fields"]
        .as_array()
        .unwrap()
        .contains(&json!("cache_read")));
    assert_eq!(detail["events"][0]["kind"], "response-final");
}

#[test]
fn replayed_strong_response_is_owned_by_root_regardless_of_import_order() {
    for child_first in [false, true] {
        let mut t = TestIndex::new();
        let root = t.root.join("parent.jsonl");
        let child = t.root.join("agent-worker.jsonl");
        let root_record = response(100, 25, 0, false);
        let mut child_record = response(100, 25, 1, true);
        child_record["agentId"] = json!("worker");
        child_record["isSidechain"] = json!(true);
        write(&root, &[root_record]);
        write(&child, &[child_record]);
        if child_first {
            t.import(&child, "child");
            t.import(&root, "parent");
        } else {
            t.import(&root, "parent");
            t.import(&child, "child");
        }
        let snapshot = t.snapshot();
        assert_eq!(
            snapshot["totals"]["total"], 125,
            "child_first={child_first}"
        );
        assert_eq!(snapshot["totals"]["events"], 1);
        let detail = t.detail("claude:parent");
        assert_eq!(detail["session"]["usage"]["total"], 125);
        assert_eq!(detail["session"]["combined_usage"]["total"], 125);
        assert_eq!(detail["events"][0]["session_id"], "claude:parent");
        assert_eq!(detail["events"][0]["kind"], "response-final");
    }
}

#[test]
fn corrected_model_prices_existing_response_without_repricing_other_history() {
    let mut t = TestIndex::new();
    let path = t.root.join("corrected-model.jsonl");
    let mut first = response(100, 10, 0, false);
    first["message"].as_object_mut().unwrap().remove("model");
    write(&path, &[first]);
    t.import(&path, "primary");
    let before = t.detail("claude:parent");
    assert_eq!(before["events"][0]["model"], "unknown");
    assert_eq!(before["events"][0]["usage"]["unpriced_tokens"], 110);
    let original_ingested = before["events"][0]["ingested_at"].clone();
    append(&path, &response(100, 25, 1, true));
    t.engine.reconcile().unwrap();
    let detail = t.detail("claude:parent");
    let event = &detail["events"][0];
    assert_eq!(event["model"], "claude-sonnet-4-6");
    assert_eq!(event["usage"]["unpriced_tokens"], 0);
    assert_eq!(event["usage"]["cost"], 0.000675);
    assert_eq!(event["ingested_at"], original_ingested);
    assert_eq!(event["pricing_version"], "bundled-2026-10-03");
    assert!(event["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap().contains("Model metadata was corrected")));
}

#[test]
fn unchanged_successful_checks_refresh_read_time_without_touching_activity_or_usage() {
    let mut t = TestIndex::new();
    let path = t.root.join("unchanged.jsonl");
    write(&path, &[response(100, 25, 0, true)]);
    t.import(&path, "primary");
    let before = t.snapshot();
    let old_read = before["sources"][0]["last_read"].clone();
    // now() stores milliseconds, so cross one timestamp tick rather than depending on scheduler timing.
    let start = std::time::Instant::now();
    while Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        == old_read.as_str().unwrap()
    {
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        std::thread::yield_now();
    }
    t.engine.reconcile().unwrap();
    let after = t.snapshot();
    assert!(after["sources"][0]["last_read"].as_str() > old_read.as_str());
    assert_eq!(
        after["sources"][0]["last_activity"],
        before["sources"][0]["last_activity"]
    );
    assert_eq!(
        after["sources"][0]["recognized"],
        before["sources"][0]["recognized"]
    );
    assert_eq!(after["totals"], before["totals"]);
}

fn codex_count(request: &str, input: u64, output: Option<u64>, seconds: i64) -> Value {
    let mut usage =
        json!({"input_tokens":input,"cached_input_tokens":0,"reasoning_output_tokens":0});
    if let Some(output) = output {
        usage["output_tokens"] = json!(output);
    }
    json!({"type":"event_msg","timestamp":(Utc::now()-Duration::minutes(5)+Duration::seconds(seconds)).to_rfc3339(),"payload":{"type":"token_count","request_id":request,"info":{"total_token_usage":usage}}})
}
fn codex_index(temp: &TempDir, path: &Path) -> Engine {
    let mut engine = Engine::isolated(&temp.path().join("codex.sqlite")).unwrap();
    engine
        .dispatch(
            "source_save",
            json!({"id":"codex","provider":"codex","path":path,"label":"Codex","enabled":true}),
        )
        .unwrap();
    engine
}
#[test]
fn known_cumulative_request_corrected_to_zero_replaces_earlier_usage_after_restart() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("codex.jsonl");
    write(
        &path,
        &[
            json!({"type":"session_meta","timestamp":Utc::now().to_rfc3339(),"payload":{"id":"corrected-zero","cwd":"/repo"}}),
            codex_count("request-a", 100, Some(10), 0),
        ],
    );
    let mut engine = codex_index(&temp, &path);
    assert_eq!(
        engine.dispatch("snapshot", json!({})).unwrap()["totals"]["total"],
        110
    );
    drop(engine);
    append(&path, &codex_count("request-a", 0, Some(0), 1));
    let mut engine = Engine::isolated(&temp.path().join("codex.sqlite")).unwrap();
    engine.reconcile().unwrap();
    let detail = engine
        .dispatch("session", json!({"id":"codex:corrected-zero"}))
        .unwrap();
    assert_eq!(detail["session"]["usage"]["total"], 0);
    assert_eq!(detail["event_count"], 1);
    assert_eq!(detail["events"][0]["usage"]["total"], 0);
    assert_eq!(detail["events"][0]["request_id"], "request-a");
    append(&path, &codex_count("request-b", 20, Some(5), 2));
    engine.reconcile().unwrap();
    assert_eq!(
        engine.dispatch("snapshot", json!({})).unwrap()["totals"]["total"],
        25
    );
}
#[test]
fn unknown_category_correction_retains_known_usage_and_counter_baseline() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("codex.jsonl");
    write(
        &path,
        &[
            json!({"type":"session_meta","timestamp":Utc::now().to_rfc3339(),"payload":{"id":"partial-correction","cwd":"/repo"}}),
            codex_count("request-a", 100, Some(10), 0),
        ],
    );
    let mut engine = codex_index(&temp, &path);
    append(&path, &codex_count("request-a", 0, None, 1));
    engine.reconcile().unwrap();
    let snapshot = engine.dispatch("snapshot", json!({})).unwrap();
    assert_eq!(snapshot["totals"]["input"], 0);
    assert_eq!(
        snapshot["totals"]["output"], 10,
        "Missing output does not imply a correction to zero"
    );
    append(&path, &codex_count("request-b", 20, Some(15), 2));
    engine.reconcile().unwrap();
    let snapshot = engine.dispatch("snapshot", json!({})).unwrap();
    assert_eq!(snapshot["totals"]["input"], 20);
    assert_eq!(
        snapshot["totals"]["output"], 15,
        "Previously observed output must not be counted again after the unknown snapshot"
    );
}

#[test]
fn zero_correction_keeps_promoted_response_identity_after_restart() {
    let temp = tempfile::TempDir::new().unwrap();
    let path = temp.path().join("promoted.jsonl");
    let response = json!({"type":"token_usage_record","timestamp":(Utc::now()-Duration::minutes(5)).to_rfc3339(),"payload":{"response_id":"upstream-response","usage":{"input_tokens":100,"output_tokens":10,"cached_input_tokens":0,"reasoning_output_tokens":0},"thread_token_usage":{"input_tokens":100,"output_tokens":10,"cached_input_tokens":0,"reasoning_output_tokens":0}}});
    write(
        &path,
        &[
            json!({"type":"session_meta","timestamp":Utc::now().to_rfc3339(),"payload":{"id":"promoted","cwd":"/repo"}}),
            response,
            codex_count("request-a", 100, Some(10), 0),
        ],
    );
    let mut engine = codex_index(&temp, &path);
    let before = engine
        .dispatch("session", json!({"id":"codex:promoted"}))
        .unwrap();
    let id = before["events"][0]["id"].clone();
    assert_eq!(before["events"][0]["request_id"], "upstream-response");
    drop(engine);
    append(&path, &codex_count("request-a", 0, Some(0), 1));
    let mut engine = Engine::isolated(&temp.path().join("codex.sqlite")).unwrap();
    engine.reconcile().unwrap();
    let after = engine
        .dispatch("session", json!({"id":"codex:promoted"}))
        .unwrap();
    assert_eq!(after["session"]["usage"]["total"], 0);
    assert_eq!(after["event_count"], 1);
    assert_eq!(after["events"][0]["id"], id);
}
