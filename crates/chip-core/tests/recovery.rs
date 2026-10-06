use chip_core::{Engine, LocalIndex, UserError};
use serde_json::{json, Value};
use std::{fs, path::Path};
use tempfile::tempdir;

fn fixture(path: &Path) {
    let timestamp = chrono::Utc::now().to_rfc3339();
    fs::write(path, format!("{}\n", json!({"type":"assistant","timestamp":timestamp,"sessionId":"recovery","cwd":"/recovery/project","requestId":"request-1","message":{"id":"message-1","model":"claude-sonnet-4-5","usage":{"input_tokens":1000,"output_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}))).unwrap();
}
fn snapshot(engine: &mut Engine) -> Value {
    engine.dispatch("snapshot", json!({})).unwrap()
}

#[test]
fn rebuild_and_upgrade_preserve_notes_prices_budgets_and_historical_estimates() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let log = dir.path().join("session.jsonl");
    fixture(&log);
    let mut engine = Engine::isolated(&path).unwrap();
    engine
        .dispatch(
            "source_save",
            json!({"provider":"claude","label":"Fixture","path":log,"enabled":true}),
        )
        .unwrap();
    let before = snapshot(&mut engine);
    let id = before["sessions"][0]["id"].as_str().unwrap();
    engine.dispatch("annotate", json!({"id":id,"alias":"Keep me","notes":"Irreplaceable notes","tags":["saved"],"pinned":true})).unwrap();
    engine.dispatch("pricing_save", json!({"model":"claude-sonnet-4-5","input":99,"output":101,"cache_read":1,"cache_write":2})).unwrap();
    engine
        .dispatch(
            "budget_save",
            json!({"name":"Keep budget","amount":100,"unit":"usd","period":"month","threshold":80}),
        )
        .unwrap();
    engine
        .dispatch(
            "project_save",
            json!({"path":"/recovery/project","notes":"Project notes"}),
        )
        .unwrap();
    let metadata = snapshot(&mut engine);
    let rebuilt = engine.dispatch("rescan", json!({"rebuild":true})).unwrap();
    let backup = rebuilt["backup"].as_str().unwrap();
    assert!(Path::new(backup).exists());
    let after = snapshot(&mut engine);
    for table in ["budgets", "prices", "projects"] {
        assert_eq!(metadata[table], after[table], "{table}");
    }
    assert_eq!(after["sessions"][0]["notes"], "Irreplaceable notes");
    assert_eq!(after["sessions"][0]["name"], "Keep me");
    assert_eq!(
        before["totals"], after["totals"],
        "Rebuild must not reprice existing usage"
    );
    let mut preserved = Engine::isolated(Path::new(backup)).unwrap();
    assert_eq!(
        snapshot(&mut preserved)["sessions"][0]["notes"],
        "Irreplaceable notes"
    );
    drop(preserved);
    drop(engine);
    // Simulate a database from before this revision, forcing the upgrade backup path.
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute("DELETE FROM config WHERE id='index_revision'", [])
        .unwrap();
    drop(db);
    let mut upgraded = Engine::isolated(&path).unwrap();
    let after_upgrade = snapshot(&mut upgraded);
    assert_eq!(metadata["prices"], after_upgrade["prices"]);
    assert_eq!(metadata["budgets"], after_upgrade["budgets"]);
    assert_eq!(after_upgrade["sessions"][0]["notes"], "Irreplaceable notes");
    assert!(fs::read_dir(dir.path()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("before-upgrade")));
}

#[test]
fn disconnected_source_refuses_rebuild_and_preserves_existing_history() {
    let dir = tempdir().unwrap();
    let log = dir.path().join("session.jsonl");
    fixture(&log);
    let mut engine = Engine::isolated(&dir.path().join("index.sqlite")).unwrap();
    engine
        .dispatch("source_save", json!({"provider":"claude","path":log}))
        .unwrap();
    let before = snapshot(&mut engine);
    fs::remove_file(&log).unwrap();
    let error = engine
        .dispatch("rescan", json!({"rebuild":true}))
        .unwrap_err();
    let error = UserError::from_error("rescan", &error);
    assert_eq!(error.category, "access_unavailable");
    assert!(error.diagnostics.contains("Backup:"));
    assert_eq!(before["totals"], snapshot(&mut engine)["totals"]);
}

#[test]
fn corrupt_startup_can_retry_preserve_and_restore_without_discarding_the_original() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("chip-count.sqlite");
    let corrupt = b"this is not a sqlite database; keep my original";
    fs::write(&db, corrupt).unwrap();
    fs::write(dir.path().join("notified-alerts.json"), b"[\"remembered\"]").unwrap();
    let mut index = LocalIndex::open(&db, false);
    let failure = index.dispatch("snapshot", json!({})).unwrap_err();
    assert_eq!(
        UserError::from_error("snapshot", &failure).category,
        "index_corrupt"
    );
    assert!(index.dispatch("index_retry", json!({})).is_err());
    let copy = index.dispatch("index_preserve", json!({})).unwrap();
    let copy = Path::new(copy["backup"].as_str().unwrap());
    assert_eq!(fs::read(copy.join("chip-count.sqlite")).unwrap(), corrupt);
    assert_eq!(fs::read(&db).unwrap(), corrupt);
    assert_eq!(
        fs::read(copy.join("notified-alerts.json")).unwrap(),
        b"[\"remembered\"]"
    );
    let good = dir.path().join("good.sqlite");
    let mut good_engine = Engine::isolated(&good).unwrap();
    good_engine.dispatch("budget_save", json!({"name":"Restored budget","amount":10,"unit":"usd","period":"month","threshold":80})).unwrap();
    let backup = good_engine.preserve("test").unwrap();
    let preserved = index.restore(&backup).unwrap();
    assert_eq!(
        fs::read(preserved.join("chip-count.sqlite")).unwrap(),
        corrupt
    );
    let data = index.dispatch("snapshot", json!({})).unwrap();
    assert_eq!(data["budgets"][0]["name"], "Restored budget");
}

#[test]
fn invalid_backup_cannot_replace_corrupt_index() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("chip-count.sqlite");
    fs::write(&db, b"keep original").unwrap();
    let invalid = dir.path().join("invalid.sqlite");
    fs::write(&invalid, b"also corrupt").unwrap();
    let mut index = LocalIndex::open(&db, false);
    assert!(index.restore(&invalid).is_err());
    assert_eq!(fs::read(&db).unwrap(), b"keep original");
}

#[test]
fn malformed_jsonl_has_file_line_and_next_steps_without_transcript_text() {
    let dir = tempdir().unwrap();
    let log = dir.path().join("bad.jsonl");
    fixture(&log);
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"PRIVATE broken content\n")
        .unwrap();
    let mut engine = Engine::isolated(&dir.path().join("index.sqlite")).unwrap();
    engine
        .dispatch("source_save", json!({"provider":"claude","path":log}))
        .unwrap();
    let data = snapshot(&mut engine);
    let source = &data["sources"][0];
    assert_eq!(source["status"], "partial");
    assert_eq!(source["diagnostics"][0]["line"], 2);
    assert_eq!(source["diagnostics"][0]["category"], "malformed_jsonl");
    assert_eq!(
        source["diagnostics"][0]["path"],
        json!(log.canonicalize().unwrap())
    );
    assert!(source["message"].as_str().unwrap().contains("rebuild"));
    assert!(!data.to_string().contains("PRIVATE"));
    assert_eq!(data["totals"]["total"], 1100);
}

#[test]
fn invalid_settings_and_rates_are_actionable_and_do_not_change_saved_values() {
    let mut engine = Engine::demo().unwrap();
    let before = snapshot(&mut engine);
    for (command, args) in [
        (
            "settings_save",
            json!({"settings":{"timezone":"Mars/Invalid"}}),
        ),
        (
            "pricing_save",
            json!({"model":"test","input":-1,"output":1,"cache_read":0,"cache_write":0}),
        ),
    ] {
        let error = engine.dispatch(command, args).unwrap_err();
        let user = UserError::from_error(command, &error);
        assert_eq!(user.category, "invalid_input");
        assert!(!user.next_steps.is_empty());
    }
    let after = snapshot(&mut engine);
    assert_eq!(before["settings"], after["settings"]);
    assert_eq!(before["prices"], after["prices"]);
}

#[test]
fn failed_write_reports_save_failure_and_retains_metadata() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let mut engine = Engine::isolated(&path).unwrap();
    let before = snapshot(&mut engine);
    // Database-level write denial is deterministic across user/root environments.
    let db =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let failure = db.execute("DELETE FROM config", []).unwrap_err();
    let user = UserError::from_error("settings_save", &failure.into());
    assert_eq!(user.category, "save_failed");
    assert!(user.next_steps.contains("retry"));
    assert_eq!(before["settings"], snapshot(&mut engine)["settings"]);
}

#[test]
fn reselecting_a_source_preserves_history_and_rejects_an_unreadable_replacement() {
    let dir = tempdir().unwrap();
    let original = dir.path().join("original.jsonl");
    fixture(&original);
    let mut engine = Engine::isolated(&dir.path().join("index.sqlite")).unwrap();
    engine
        .dispatch("source_save", json!({"provider":"claude","path":original}))
        .unwrap();
    let before = snapshot(&mut engine);
    let source_id = before["sources"][0]["id"].as_str().unwrap();
    let session_id = before["sessions"][0]["id"].as_str().unwrap();
    engine
        .dispatch(
            "annotate",
            json!({"id":session_id,"notes":"Keep after reselect"}),
        )
        .unwrap();
    let missing = dir.path().join("missing.jsonl");
    assert!(engine
        .dispatch(
            "source_save",
            json!({"id":source_id,"provider":"claude","path":missing})
        )
        .is_err());
    assert_eq!(
        snapshot(&mut engine)["sources"][0]["path"],
        json!(original.canonicalize().unwrap())
    );
    let replacement = dir.path().join("replacement.jsonl");
    fs::write(&replacement, b"").unwrap();
    let result = engine
        .dispatch(
            "source_save",
            json!({"id":source_id,"provider":"claude","path":replacement}),
        )
        .unwrap();
    assert!(Path::new(result["backup"].as_str().unwrap()).exists());
    let after = snapshot(&mut engine);
    assert_eq!(before["totals"], after["totals"]);
    assert_eq!(after["sessions"][0]["notes"], "Keep after reselect");
    assert!(after["sources"][0]["recovery_backup"].is_string());
}
