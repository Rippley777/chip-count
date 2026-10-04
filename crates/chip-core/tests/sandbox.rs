use chip_core::Engine;
use serde_json::{json, Value};
use std::{collections::HashMap, fs, path::PathBuf};
static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "chip-sandbox-{}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        fs::create_dir_all(root.join("logs")).unwrap();
        Self {
            root: root.canonicalize().unwrap(),
        }
    }
    fn db(&self) -> PathBuf {
        self.root.join("index.sqlite")
    }
    fn log(&self) -> PathBuf {
        self.root.join("logs/session.jsonl")
    }
    fn source(&self) -> Value {
        json!({"id":"selected","path":self.root.join("logs"),"provider":"claude","enabled":true})
    }
    fn grants(&self) -> HashMap<String, PathBuf> {
        HashMap::from([("selected".into(), self.root.join("logs"))])
    }
    fn snapshot(&self, e: &mut Engine) -> Value {
        e.dispatch("snapshot", json!({})).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn record(id: &str, input: u64) -> String {
    format!(
        "{}\n",
        json!({"type":"assistant","sessionId":"sandbox-session","uuid":id,"timestamp":chrono::Utc::now().to_rfc3339(),"message":{"id":id,"role":"assistant","model":"claude-sonnet-4-6","usage":{"input_tokens":input,"output_tokens":10}}})
    )
}
#[test]
fn sandbox_rejects_raw_paths_and_restored_paths_until_native_grant() {
    let f = Fixture::new();
    fs::write(f.log(), record("a", 100)).unwrap();
    // A pre-existing path-based index must not acquire sandbox permission on migration.
    let mut old = Engine::isolated(&f.db()).unwrap();
    old.dispatch("source_save", f.source()).unwrap();
    drop(old);
    let mut e = Engine::sandboxed(&f.db()).unwrap();
    assert_eq!(e.source_records().unwrap().len(), 1, "No auto-discovery");
    assert!(e.watch_roots().is_empty());
    assert!(e.dispatch("source_save", f.source()).is_err());
    e.reconcile().unwrap();
    assert_eq!(f.snapshot(&mut e)["sources"][0]["status"], "unavailable");
    e.set_source_grants(f.grants());
    e.reconcile().unwrap();
    assert_eq!(e.watch_roots(), vec![f.root.join("logs")]);
    assert_ne!(f.snapshot(&mut e)["sources"][0]["status"], "unavailable");
    let mut tampered = f.source();
    tampered["path"] = json!(f.root);
    assert!(e.dispatch("source_save", tampered).is_err());
}
#[test]
fn sandbox_restart_revoke_regrant_append_rotate_and_rebuild_preserve_logs() {
    let f = Fixture::new();
    fs::write(f.log(), record("a", 100)).unwrap();
    let mut e = Engine::sandboxed(&f.db()).unwrap();
    e.set_source_grants(f.grants());
    e.dispatch("source_save", f.source()).unwrap();
    e.save_source_bookmark("selected", &[1, 2, 3]).unwrap();
    let before = fs::read(f.log()).unwrap();
    e.reconcile().unwrap();
    assert_eq!(fs::read(f.log()).unwrap(), before);
    assert_eq!(f.snapshot(&mut e)["totals"]["total"], 110);
    drop(e);
    let mut e = Engine::sandboxed(&f.db()).unwrap();
    assert_eq!(e.source_bookmark("selected").unwrap(), Some(vec![1, 2, 3]));
    e.reconcile().unwrap();
    assert_eq!(f.snapshot(&mut e)["sources"][0]["status"], "unavailable");
    e.set_source_grants(f.grants());
    use std::io::Write;
    fs::OpenOptions::new()
        .append(true)
        .open(f.log())
        .unwrap()
        .write_all(record("b", 200).as_bytes())
        .unwrap();
    let appended = fs::read(f.log()).unwrap();
    e.reconcile().unwrap();
    assert_eq!(f.snapshot(&mut e)["totals"]["total"], 320);
    assert_eq!(fs::read(f.log()).unwrap(), appended);
    fs::rename(f.log(), f.root.join("logs/rotated.jsonl")).unwrap();
    fs::write(f.log(), record("c", 300)).unwrap();
    e.reconcile().unwrap();
    assert_eq!(f.snapshot(&mut e)["totals"]["total"], 630);
    e.set_source_grants(HashMap::new());
    e.reconcile().unwrap();
    assert!(e.watch_roots().is_empty());
    assert_eq!(f.snapshot(&mut e)["sources"][0]["status"], "unavailable");
    let rotated = fs::read(f.root.join("logs/rotated.jsonl")).unwrap();
    let current = fs::read(f.log()).unwrap();
    e.set_source_grants(f.grants());
    e.dispatch("rescan", json!({"rebuild":true})).unwrap();
    assert_eq!(f.snapshot(&mut e)["totals"]["total"], 630);
    assert_eq!(fs::read(f.log()).unwrap(), current);
    assert_eq!(
        fs::read(f.root.join("logs/rotated.jsonl")).unwrap(),
        rotated
    );
    assert!(
        !f.snapshot(&mut e).to_string().contains("bookmark"),
        "Bookmarks never enter IPC snapshots"
    );
    e.dispatch("source_remove", json!({"id":"selected"}))
        .unwrap();
    assert!(e.source_bookmark("selected").unwrap().is_none());
}
