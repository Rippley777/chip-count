use chip_core::Engine;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;
fn invoke(e: &mut Engine, command: &str, args: Value) -> Value {
    e.dispatch(command, args).unwrap()
}
fn record(session: &str, request: &str, timestamp: &str, tokens: u64, project: &str) -> Value {
    json!({"type":"assistant","sessionId":session,"cwd":project,"timestamp":timestamp,"requestId":request,"message":{"id":request,"role":"assistant","stop_reason":"end_turn","model":"claude-sonnet-4-6","usage":{"input_tokens":tokens,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}})
}
fn engine(records: &[Value]) -> (TempDir, Engine) {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("usage.jsonl");
    fs::write(
        &path,
        records.iter().map(|x| format!("{x}\n")).collect::<String>(),
    )
    .unwrap();
    let mut e = Engine::isolated(&tmp.path().join("index.sqlite")).unwrap();
    invoke(
        &mut e,
        "settings_save",
        json!({"settings":{"retention_days":36500}}),
    );
    invoke(
        &mut e,
        "source_save",
        json!({"provider":"claude","label":"Testing","path":path,"enabled":true}),
    );
    (tmp, e)
}
#[test]
fn timezone_day_filter_handles_spring_forward_23_hours() {
    let records = [
        record("session", "before", "2026-03-08T05:59:59Z", 1, "/repo"),
        record("session", "first", "2026-03-08T06:00:00Z", 2, "/repo"),
        record("session", "last", "2026-03-09T04:59:59Z", 4, "/repo"),
        record("session", "after", "2026-03-09T05:00:00Z", 8, "/repo"),
    ];
    let (_tmp, mut e) = engine(&records);
    invoke(
        &mut e,
        "settings_save",
        json!({"settings":{"timezone":"America/Chicago"}}),
    );
    let s = invoke(
        &mut e,
        "snapshot",
        json!({"filter":{"from":"2026-03-08","to":"2026-03-08"}}),
    );
    assert_eq!(s["totals"]["total"], 6);
    assert_eq!(s["daily"].as_array().unwrap().len(), 1);
    assert_eq!(s["daily"][0]["key"], "2026-03-08");
}
#[test]
fn timezone_day_filter_handles_fall_back_repeated_hour() {
    let records = [
        record("session", "first", "2026-11-01T06:30:00Z", 3, "/repo"),
        record("session", "second", "2026-11-01T07:30:00Z", 7, "/repo"),
        record("session", "end", "2026-11-02T05:59:59Z", 11, "/repo"),
        record("session", "next", "2026-11-02T06:00:00Z", 13, "/repo"),
    ];
    let (_tmp, mut e) = engine(&records);
    invoke(
        &mut e,
        "settings_save",
        json!({"settings":{"timezone":"America/Chicago"}}),
    );
    let s = invoke(
        &mut e,
        "snapshot",
        json!({"filter":{"from":"2026-11-01","to":"2026-11-01"}}),
    );
    assert_eq!(s["totals"]["total"], 21);
    let detail = invoke(
        &mut e,
        "session",
        json!({"id":"claude:session","filter":{"from":"2026-11-01","to":"2026-11-01"}}),
    );
    let keys: Vec<_> = detail["timeline"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["key"].as_str())
        .collect();
    assert!(keys.iter().any(|x| x.ends_with("-05:00")));
    assert!(keys.iter().any(|x| x.ends_with("-06:00")));
}
#[test]
fn budget_alerts_persist_deduplicate_and_ignore_view_filter() {
    let now = Utc::now();
    let records = [
        record(
            "one",
            "a",
            &(now - Duration::minutes(3)).to_rfc3339(),
            50,
            "/one",
        ),
        record(
            "two",
            "b",
            &(now - Duration::minutes(2)).to_rfc3339(),
            30,
            "/two",
        ),
    ];
    let (_tmp, mut e) = engine(&records);
    invoke(
        &mut e,
        "budget_save",
        json!({"id":"daily","name":"Daily","amount":100.,"unit":"tokens","period":"day","threshold":75.}),
    );
    let first = invoke(&mut e, "snapshot", json!({"filter":{"project":"/one"}}));
    let second = invoke(&mut e, "snapshot", json!({"filter":{"project":"/two"}}));
    assert_eq!(first["totals"]["total"], 50);
    assert_eq!(first["budgets"][0]["used"], 80.);
    assert_eq!(first["alerts"].as_array().unwrap().len(), 1);
    assert_eq!(second["alerts"].as_array().unwrap().len(), 1);
    assert_eq!(second["alerts"][0]["id"], first["alerts"][0]["id"]);
}
#[test]
fn budget_projects_respect_explicit_aliases_without_basename_merging() {
    let now = Utc::now().to_rfc3339();
    let records = [
        record("one", "a", &now, 50, "/one/repo"),
        record("two", "b", &now, 30, "/two/repo"),
        record("three", "c", &now, 20, "/worktree"),
    ];
    let (_tmp, mut e) = engine(&records);
    invoke(
        &mut e,
        "project_save",
        json!({"path":"/one/repo","name":"Primary","aliases":["/worktree"]}),
    );
    invoke(
        &mut e,
        "budget_save",
        json!({"id":"project","name":"Primary","project":"/one/repo","amount":100.,"unit":"tokens","period":"day","threshold":90.}),
    );
    let s = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(s["projects"].as_array().unwrap().len(), 2);
    assert_eq!(s["budgets"][0]["used"], 70.);
    let filtered = invoke(
        &mut e,
        "snapshot",
        json!({"filter":{"project":"/one/repo"}}),
    );
    assert_eq!(filtered["totals"]["total"], 70);
}
#[test]
fn compare_and_json_export_agree_with_filtered_totals() {
    let now = Utc::now().to_rfc3339();
    let records = [
        record("one", "a", &now, 50, "/one"),
        record("two", "b", &now, 30, "/two"),
    ];
    let (_tmp, mut e) = engine(&records);
    let s = invoke(&mut e, "snapshot", json!({"filter":{"project":"/one"}}));
    let c = invoke(
        &mut e,
        "compare",
        json!({"ids":["claude:one","claude:two"],"alignment":"elapsed"}),
    );
    assert_eq!(c["items"][0]["usage"], s["totals"]);
    let export = invoke(
        &mut e,
        "export",
        json!({"format":"json","filter":{"project":"/one"},"redact_paths":true,"redact_labels":true}),
    );
    let d: Value = serde_json::from_str(export["content"].as_str().unwrap()).unwrap();
    assert_eq!(d["totals"], s["totals"]);
    assert_eq!(d["events"].as_array().unwrap().len(), 1);
    assert!(!export["content"].as_str().unwrap().contains("/one"));
    assert!(!export["content"].as_str().unwrap().contains("Testing"));
}
#[test]
fn session_pagination_does_not_change_global_totals() {
    let mut e = Engine::demo().unwrap();
    let all = invoke(&mut e, "snapshot", json!({}));
    let paged = invoke(
        &mut e,
        "snapshot",
        json!({"filter":{"offset":2,"limit":3,"sort":"cost"}}),
    );
    assert_eq!(paged["sessions"].as_array().unwrap().len(), 3);
    assert_eq!(paged["totals"], all["totals"]);
    assert_eq!(paged["total_sessions"], all["total_sessions"]);
}
#[test]
fn rebuild_demo_keeps_demo_functional() {
    let mut e = Engine::demo().unwrap();
    let before = invoke(&mut e, "snapshot", json!({}));
    invoke(&mut e, "rescan", json!({"rebuild":true}));
    let after = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(before["totals"], after["totals"]);
    assert_eq!(after["demo"], true);
    assert!(after["active_sessions"].as_u64().unwrap() > 0);
}
#[test]
fn invalid_settings_and_budget_inputs_are_rejected() {
    let mut e = Engine::demo().unwrap();
    for value in [
        json!({"timezone":"Mars/Olympus"}),
        json!({"inactivity_minutes":0}),
        json!({"retention_days":-1}),
        json!({"notifications":"yes"}),
    ] {
        assert!(e
            .dispatch("settings_save", json!({"settings":value}))
            .is_err());
    }
    assert!(e
        .dispatch(
            "budget_save",
            json!({"name":"bad","amount":0,"unit":"tokens","period":"day","threshold":50})
        )
        .is_err());
}

#[test]
fn copied_logs_keep_profile_membership_when_original_source_is_disabled() {
    let temp = TempDir::new().unwrap();
    let work = temp.path().join("work.jsonl");
    let personal = temp.path().join("personal.jsonl");
    let now = Utc::now().to_rfc3339();
    let shared = record("one", "shared", &now, 50, "/repo");
    let work_only = record("one", "work-only", &now, 20, "/repo");
    fs::write(&work, format!("{shared}\n{work_only}\n")).unwrap();
    fs::write(&personal, format!("{shared}\n")).unwrap();
    let mut e = Engine::isolated(&temp.path().join("index.sqlite")).unwrap();
    invoke(
        &mut e,
        "source_save",
        json!({"id":"work","provider":"claude","label":"Work","path":work,"enabled":true}),
    );
    invoke(
        &mut e,
        "source_save",
        json!({"id":"personal","provider":"claude","label":"Personal","path":personal,"enabled":true}),
    );
    let work_view = invoke(&mut e, "snapshot", json!({"filter":{"profile":"Work"}}));
    let personal_view = invoke(&mut e, "snapshot", json!({"filter":{"profile":"Personal"}}));
    let all = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(work_view["totals"]["total"], 70);
    assert_eq!(personal_view["totals"]["total"], 50);
    assert_eq!(all["totals"]["total"], 70);
    invoke(
        &mut e,
        "source_save",
        json!({"id":"work","provider":"claude","label":"Work","path":work,"enabled":false}),
    );
    let remaining = invoke(&mut e, "snapshot", json!({"filter":{"profile":"Personal"}}));
    assert_eq!(remaining["totals"]["total"], 50);
    assert_eq!(remaining["sessions"][0]["profile"], "Personal");
}
#[test]
fn custom_window_budget_counts_only_its_window_and_validates_bounds() {
    let now = Utc::now();
    let records = [
        record(
            "one",
            "old",
            &(now - Duration::minutes(70)).to_rfc3339(),
            100,
            "/repo",
        ),
        record(
            "one",
            "new",
            &(now - Duration::minutes(5)).to_rfc3339(),
            20,
            "/repo",
        ),
    ];
    let (_tmp, mut e) = engine(&records);
    invoke(
        &mut e,
        "budget_save",
        json!({"id":"custom","name":"An hour","amount":100.,"unit":"tokens","period":"window","window_minutes":60,"threshold":80.}),
    );
    let snap = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(snap["budgets"][0]["used"], 20.);
    assert_eq!(snap["budgets"][0]["window_minutes"], 60);
    for minutes in [json!(0), json!(525601), json!(1.5)] {
        assert!(e.dispatch("budget_save",json!({"name":"Invalid","amount":100.,"unit":"tokens","period":"window","window_minutes":minutes,"threshold":80.})).is_err());
    }
}
#[test]
fn fractional_token_rate_rounds_once_and_export_preserves_pricing_versions() {
    let now = Utc::now().to_rfc3339();
    let (_tmp, mut e) = engine(&[record("one", "a", &now, 1_000_000, "/repo")]);
    let original = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(original["totals"]["cost"], 3.);
    invoke(
        &mut e,
        "pricing_save",
        json!({"model":"claude-sonnet-4-6","input":0.0001,"output":0,"cache_read":0,"cache_write":0}),
    );
    let unchanged = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(unchanged["totals"]["cost"], 3.);
    invoke(&mut e, "reprice", json!({}));
    let changed = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(changed["totals"]["cost"], 0.0001);
    let exported = invoke(
        &mut e,
        "export",
        json!({"format":"json","redact_paths":false,"redact_labels":false}),
    );
    let data: Value = serde_json::from_str(exported["content"].as_str().unwrap()).unwrap();
    assert!(data["pricing_history"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["model"] == "claude-sonnet-4-6" && p["version"] == "bundled-2026-10-03"));
    assert!(data["pricing_history"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["input"] == 0.0001));
}
#[test]
fn demo_rebuild_preserves_local_annotations_and_removed_budget() {
    let mut e = Engine::demo().unwrap();
    invoke(
        &mut e,
        "annotate",
        json!({"id":"claude:demo-00-0","alias":"My custom name","notes":"Keep this note","pinned":false}),
    );
    invoke(&mut e, "budget_remove", json!({"id":"demo-day"}));
    invoke(&mut e, "rescan", json!({"rebuild":true}));
    let detail = invoke(&mut e, "session", json!({"id":"claude:demo-00-0"}));
    assert_eq!(detail["session"]["name"], "My custom name");
    assert_eq!(detail["session"]["notes"], "Keep this note");
    let snap = invoke(&mut e, "snapshot", json!({}));
    assert!(!snap["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|b| b["id"] == "demo-day"));
}

#[test]
fn started_session_is_visible_before_its_first_usage_report() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("new-session.jsonl");
    let now = Utc::now().to_rfc3339();
    let rows = [
        json!({"type":"session_meta","timestamp":now,"payload":{"id":"just-started","cwd":"/repo"}}),
        json!({"type":"event_msg","timestamp":now,"payload":{"type":"task_started","turn_id":"first"}}),
    ];
    fs::write(
        &path,
        rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    let mut e = Engine::isolated(&temp.path().join("db.sqlite")).unwrap();
    invoke(
        &mut e,
        "source_save",
        json!({"provider":"codex","label":"Fresh","path":path,"enabled":true}),
    );
    let s = invoke(&mut e, "snapshot", json!({}));
    assert_eq!(s["total_sessions"], 1);
    assert_eq!(s["sessions"][0]["state"], "active");
    assert_eq!(s["sessions"][0]["usage"]["total"], 0);
    assert_eq!(s["sessions"][0]["profile"], "Fresh");
}

#[test]
fn session_event_search_reaches_beyond_page_without_changing_totals() {
    let now = Utc::now();
    let mut records: Vec<_> = (0..205)
        .map(|i| {
            record(
                "large",
                &format!("request-{i}"),
                &(now + Duration::seconds(i)).to_rfc3339(),
                1,
                "/large",
            )
        })
        .collect();
    records[0]["message"]["model"] = json!("rare-model-at-start");
    let (_tmp, mut engine) = engine(&records);
    let first = invoke(&mut engine, "session", json!({"id":"claude:large"}));
    assert_eq!(first["event_count"], 205);
    assert_eq!(first["event_matches"], 205);
    assert_eq!(first["events"].as_array().unwrap().len(), 200);
    let second = invoke(
        &mut engine,
        "session",
        json!({"id":"claude:large","event_offset":200}),
    );
    assert_eq!(second["events"].as_array().unwrap().len(), 5);
    assert!(second["events"]
        .as_array()
        .unwrap()
        .iter()
        .all(|e| !first["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["id"] == e["id"])));
    let found = invoke(
        &mut engine,
        "session",
        json!({"id":"claude:large","event_search":"rare-model-at-start"}),
    );
    assert_eq!(found["event_matches"], 1);
    assert_eq!(found["events"][0]["model"], "rare-model-at-start");
    assert_eq!(found["session"]["usage"], first["session"]["usage"]);
    assert_eq!(found["timeline"], first["timeline"]);
}

#[test]
fn comparison_items_preserve_identity_when_filter_omits_first_selection() {
    let now = Utc::now().to_rfc3339();
    let (_tmp, mut engine) = engine(&[
        record("one", "one", &now, 1, "/one"),
        record("two", "two", &now, 2, "/two"),
    ]);
    let compared = invoke(
        &mut engine,
        "compare",
        json!({"ids":["claude:one","claude:two"],"filter":{"project":"/two"}}),
    );
    assert_eq!(compared["items"].as_array().unwrap().len(), 1);
    assert_eq!(compared["items"][0]["id"], "claude:two");
}

#[test]
fn unpriced_budget_reports_incomplete_coverage_instead_of_implied_zero_cost() {
    let now = Utc::now();
    let records: Vec<_> = (1..=3)
        .map(|i| {
            let mut r = record(
                "unpriced",
                &format!("unknown-{i}"),
                &(now - Duration::minutes(i)).to_rfc3339(),
                100,
                "/repo",
            );
            r["message"]["model"] = json!("unpriced-experimental-model");
            r
        })
        .collect();
    let (_tmp, mut e) = engine(&records);
    invoke(
        &mut e,
        "budget_save",
        json!({"id":"usd","name":"Dollar budget","amount":1.,"unit":"usd","period":"5h","threshold":80.}),
    );
    let snapshot = invoke(&mut e, "snapshot", json!({}));
    let budget = &snapshot["budgets"][0];
    assert_eq!(budget["used"], 0.);
    assert_eq!(budget["unpriced_tokens"], 300);
    assert_eq!(budget["unknown_fields"], json!([]));
    assert!(budget["projected_at"].is_null());
}

#[test]
fn incomplete_pricing_anywhere_in_budget_window_suppresses_forecast_but_keeps_alerts() {
    let now = Utc::now();
    let mut records: Vec<_> = (1..=3)
        .map(|i| {
            record(
                "mixed",
                &format!("known-{i}"),
                &(now - Duration::minutes(i)).to_rfc3339(),
                100,
                "/repo",
            )
        })
        .collect();
    let mut unknown = record(
        "mixed",
        "older-unknown",
        &(now - Duration::minutes(45)).to_rfc3339(),
        100,
        "/repo",
    );
    unknown["message"]["model"] = json!("unpriced-experimental-model");
    records.push(unknown);
    let (_tmp, mut e) = engine(&records);
    for (id, amount) in [("forecast", 1.), ("alert", 0.0001)] {
        invoke(
            &mut e,
            "budget_save",
            json!({"id":id,"name":id,"amount":amount,"unit":"usd","period":"5h","threshold":80.}),
        );
    }
    let snapshot = invoke(&mut e, "snapshot", json!({}));
    let budget = snapshot["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "forecast")
        .unwrap();
    assert_eq!(budget["used"], 0.0009);
    assert_eq!(budget["unpriced_tokens"], 100);
    assert!(
        budget["projected_at"].is_null(),
        "Older unpriced records in the active window still make the total incomplete"
    );
    assert!(
        snapshot["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["budget_id"] == "alert"),
        "Known usage crossing a threshold remains actionable even when the total is a lower bound"
    );
}

#[test]
fn missing_total_category_suppresses_both_token_and_dollar_budget_forecasts() {
    let now = Utc::now();
    let records: Vec<_> = (1..=3)
        .map(|i| {
            let mut r = record(
                "partial",
                &format!("partial-{i}"),
                &(now - Duration::minutes(i)).to_rfc3339(),
                100,
                "/repo",
            );
            r["message"]["usage"]
                .as_object_mut()
                .unwrap()
                .remove("output_tokens");
            r
        })
        .collect();
    let (_tmp, mut e) = engine(&records);
    for (unit, amount) in [("usd", 1.), ("tokens", 1000.)] {
        invoke(
            &mut e,
            "budget_save",
            json!({"id":unit,"name":unit,"amount":amount,"unit":unit,"period":"5h","threshold":80.}),
        );
    }
    let snapshot = invoke(&mut e, "snapshot", json!({}));
    for budget in snapshot["budgets"].as_array().unwrap() {
        assert_eq!(budget["unpriced_tokens"], 0);
        assert!(budget["unknown_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("output")));
        assert!(budget["projected_at"].is_null());
        assert!(
            budget["used"].as_f64().unwrap() > 0.,
            "Known categories remain a reported lower bound"
        );
    }
}

#[test]
fn missing_reasoning_subset_does_not_prevent_token_budget_forecast() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("codex.jsonl");
    let now = Utc::now();
    let mut records = vec![
        json!({"type":"session_meta","timestamp":(now-Duration::minutes(5)).to_rfc3339(),"payload":{"id":"reasoning-unknown","cwd":"/repo"}}),
        json!({"type":"turn_context","timestamp":(now-Duration::minutes(5)).to_rfc3339(),"payload":{"model":"gpt-5.3-codex"}}),
    ];
    for i in 1..=3 {
        records.push(json!({"type":"event_msg","timestamp":(now-Duration::minutes(4-i)).to_rfc3339(),"payload":{"type":"token_count","request_id":format!("request-{i}"),"info":{"total_token_usage":{"input_tokens":i*100,"output_tokens":i*10,"cached_input_tokens":0}}}}));
    }
    fs::write(
        &path,
        records.iter().map(|r| format!("{r}\n")).collect::<String>(),
    )
    .unwrap();
    let mut e = Engine::isolated(&temp.path().join("index.sqlite")).unwrap();
    invoke(
        &mut e,
        "source_save",
        json!({"provider":"codex","label":"Subset test","path":path,"enabled":true}),
    );
    for (unit, amount) in [("usd", 1.), ("tokens", 1000.)] {
        invoke(
            &mut e,
            "budget_save",
            json!({"id":unit,"name":unit,"amount":amount,"unit":unit,"period":"5h","threshold":80.}),
        );
    }
    let snapshot = invoke(&mut e, "snapshot", json!({}));
    let tokens = snapshot["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "tokens")
        .unwrap();
    let usd = snapshot["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "usd")
        .unwrap();
    assert_eq!(tokens["used"], 330.);
    assert_eq!(tokens["unknown_fields"], json!(["reasoning"]));
    assert!(
        tokens["projected_at"].is_string(),
        "Reasoning is a subset of already-counted output"
    );
    assert!(usd["projected_at"].is_null());
}

#[test]
fn unbounded_history_has_no_previous_period_but_bounded_comparison_matches_span() {
    let records = [
        record("before", "a", "2026-09-01T12:00:00Z", 5, "/repo"),
        record("current", "b", "2026-09-02T12:00:00Z", 10, "/repo"),
    ];
    let (_tmp, mut engine) = engine(&records);
    let all = invoke(&mut engine, "snapshot", json!({}));
    assert!(all["previous"].is_null());
    let day = invoke(
        &mut engine,
        "snapshot",
        json!({"filter":{"from":"2026-09-02","to":"2026-09-02"}}),
    );
    assert_eq!(day["totals"]["total"], 10);
    assert_eq!(day["previous"]["total"], 5);
}
