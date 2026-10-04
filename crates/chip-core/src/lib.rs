//! Local, metadata-only accounting engine. No network access and no dependency on Tauri.
mod analytics;
mod calendar;
mod demo;
mod ingest;
pub mod model;
mod parser;
mod pricing;

use anyhow::{bail, Context, Result};
use chrono_tz::Tz;
use model::*;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

pub struct Engine {
    pub(crate) db: Connection,
    pub(crate) is_demo: bool,
    // None is the development/direct-distribution policy. Sandbox grants are runtime-only.
    pub(crate) source_grants: Option<HashMap<String, PathBuf>>,
}
impl Engine {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|x| !x.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut engine = Self::init(Connection::open(path)?, false)?;
        engine.discover()?;
        Ok(engine)
    }
    /// No environment or home discovery; every external source requires a native grant.
    pub fn sandboxed(path: &Path) -> Result<Self> {
        let mut engine = Self::init(Connection::open(path)?, false)?;
        engine.source_grants = Some(HashMap::new());
        Ok(engine)
    }
    pub fn source_records(&self) -> Result<Vec<Value>> {
        self.list("sources")
    }
    pub fn set_source_grants(&mut self, grants: HashMap<String, PathBuf>) {
        self.source_grants = Some(grants);
    }
    pub fn source_bookmark(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.get("config", &format!("bookmark:{id}"))?
            .map(|v| serde_json::from_value(v).map_err(Into::into))
            .transpose()
    }
    pub fn save_source_bookmark(&self, id: &str, bookmark: &[u8]) -> Result<()> {
        self.put("config", &format!("bookmark:{id}"), &json!(bookmark))
    }
    /// Commit a native selection and its bookmark together, before any scan.
    pub fn save_bookmarked_source(&self, args: Value, bookmark: &[u8]) -> Result<()> {
        let id = required(&args, "id")?.to_owned();
        self.db.execute_batch("SAVEPOINT native_source")?;
        let result = self
            .save_source(args, false)
            .and_then(|()| self.save_source_bookmark(&id, bookmark));
        if result.is_err() {
            self.db.execute_batch("ROLLBACK TO native_source")?;
        }
        self.db.execute_batch("RELEASE native_source")?;
        result
    }
    /// Called only by the native bookmark resolver while its scope is held.
    pub fn relocate_source(&self, id: &str, path: &Path) -> Result<()> {
        if let Some(mut source) = self.get("sources", id)? {
            source["path"] = json!(path);
            self.put("sources", id, &source)?;
        }
        Ok(())
    }
    pub fn demo() -> Result<Self> {
        let mut e = Self::init(Connection::open_in_memory()?, true)?;
        e.populate_demo()?;
        Ok(e)
    }
    pub fn isolated(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?, false)
    }
    fn init(db: Connection, is_demo: bool) -> Result<Self> {
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;
          CREATE TABLE IF NOT EXISTS schema_version(version INTEGER NOT NULL); INSERT INTO schema_version SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM schema_version);
          CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS files(path TEXT NOT NULL, source_id TEXT NOT NULL, offset INTEGER NOT NULL, line INTEGER NOT NULL, length INTEGER NOT NULL, fingerprint TEXT NOT NULL, state TEXT NOT NULL, PRIMARY KEY(path,source_id));
          CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS events(id TEXT PRIMARY KEY, session_id TEXT NOT NULL, timestamp TEXT NOT NULL, model TEXT NOT NULL, scope TEXT NOT NULL, data TEXT NOT NULL, nano INTEGER NOT NULL DEFAULT 0);
          CREATE INDEX IF NOT EXISTS events_time ON events(timestamp); CREATE INDEX IF NOT EXISTS events_session ON events(session_id,timestamp); CREATE INDEX IF NOT EXISTS events_model ON events(model,timestamp);
          CREATE TABLE IF NOT EXISTS origins(event_id TEXT NOT NULL, path TEXT NOT NULL, source_id TEXT NOT NULL, PRIMARY KEY(event_id,path,source_id)); CREATE INDEX IF NOT EXISTS origins_source ON origins(source_id);
          CREATE TABLE IF NOT EXISTS markers(id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS annotations(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS budgets(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS alerts(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS limits(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS prices(id TEXT PRIMARY KEY, data TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS config(id TEXT PRIMARY KEY, data TEXT NOT NULL);")?;
        let e = Self {
            db,
            is_demo,
            source_grants: None,
        };
        if e.get("config", "settings")?.is_none() {
            e.put("config", "settings", &settings_default())?;
        }
        e.seed_prices()?;
        Ok(e)
    }
    pub(crate) fn get(&self, table: &str, id: &str) -> Result<Option<Value>> {
        use rusqlite::OptionalExtension;
        let s: Option<String> = self
            .db
            .query_row(
                &format!("SELECT data FROM {table} WHERE id=?1"),
                [id],
                |r| r.get(0),
            )
            .optional()?;
        s.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub(crate) fn put(&self, table: &str, id: &str, v: &Value) -> Result<()> {
        self.db.execute(&format!("INSERT INTO {table}(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data"),params![id,v.to_string()])?;
        Ok(())
    }
    pub(crate) fn list(&self, table: &str) -> Result<Vec<Value>> {
        let mut q = self
            .db
            .prepare(&format!("SELECT data FROM {table} ORDER BY id"))?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = vec![];
        for row in rows {
            out.push(serde_json::from_str(&row?)?);
        }
        Ok(out)
    }
    pub(crate) fn settings(&self) -> Result<Value> {
        Ok(self
            .get("config", "settings")?
            .unwrap_or_else(settings_default))
    }
    pub fn watch_roots(&self) -> Vec<PathBuf> {
        self.list("sources")
            .unwrap_or_default()
            .iter()
            .filter(|s| s["enabled"] == true && self.source_authorized(s))
            .filter_map(|s| s["path"].as_str().map(PathBuf::from))
            .collect()
    }
    pub(crate) fn source_authorized(&self, source: &Value) -> bool {
        self.source_grants.as_ref().is_none_or(|grants| {
            source["id"]
                .as_str()
                .and_then(|id| grants.get(id))
                .is_some_and(|path| {
                    source["path"]
                        .as_str()
                        .is_some_and(|s| path == Path::new(s))
                })
        })
    }
    fn discover(&mut self) -> Result<()> {
        if self.get("config", "discovered")?.is_some() {
            return Ok(());
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let mut roots: Vec<(&str, PathBuf)> = vec![];
        if let Ok(config) = std::env::var("CLAUDE_CONFIG_DIR") {
            for root in config.split(',').filter(|x| !x.trim().is_empty()) {
                roots.push(("claude", PathBuf::from(root.trim()).join("projects")));
            }
        } else {
            roots.push(("claude", home.join(".claude/projects")));
            roots.push(("claude", home.join(".config/claude/projects")));
        }
        let codex_roots = std::env::var("CODEX_HOME")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|r| PathBuf::from(r.trim()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![home.join(".codex")]);
        for codex in codex_roots {
            roots.push(("codex", codex.join("sessions")));
            roots.push(("codex", codex.join("archived_sessions")));
        }
        for (provider, path) in roots {
            self.save_source(json!({"provider":provider,"label":if provider=="claude"{"Claude Code"}else{"Codex"},"path":path,"enabled":true}),true)?;
        }
        self.put("config", "discovered", &json!(true))?;
        Ok(())
    }
    fn save_source(&self, args: Value, discovery: bool) -> Result<()> {
        let provider = required(&args, "provider")?;
        if provider != "claude" && provider != "codex" {
            bail!("Choose Claude or Codex as the source provider");
        }
        let raw = required(&args, "path")?;
        if raw.contains('\0') {
            bail!("Invalid source path");
        }
        let path = Path::new(raw);
        if !path.is_absolute() {
            bail!("Select an absolute folder or JSONL path");
        }
        if !discovery
            && path.exists()
            && !path.is_dir()
            && path.extension().and_then(|x| x.to_str()) != Some("jsonl")
        {
            bail!("Select a directory or .jsonl file");
        }
        if args.get("exclusions").is_some() && !args["exclusions"].is_array() {
            bail!("Exclusions must be an array of glob patterns");
        }
        if let Some(patterns) = args["exclusions"].as_array() {
            for pattern in patterns {
                globset::Glob::new(
                    pattern
                        .as_str()
                        .context("Exclusions must be glob strings")?,
                )
                .context("Invalid exclusion glob")?;
            }
        }
        let canonical = std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_owned())
            .to_string_lossy()
            .to_string();
        let id = args["id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| hash(&format!("{provider}:{canonical}")));
        let old = self.get("sources", &id)?;
        let disabling_existing =
            args["enabled"] == false && old.as_ref().is_some_and(|s| s["path"] == canonical);
        if !disabling_existing && !self.source_authorized(&json!({"id":id,"path":canonical})) {
            bail!("Choose this source with the native folder or JSONL picker to grant access.");
        }
        let changed = old.as_ref().is_some_and(|s| {
            s["path"] != canonical
                || s["provider"] != provider
                || args
                    .get("exclusions")
                    .is_some_and(|e| s["exclusions"] != *e)
        });
        if changed {
            self.db
                .execute("DELETE FROM origins WHERE source_id=?1", [&id])?;
            self.db
                .execute("DELETE FROM files WHERE source_id=?1", [&id])?;
            self.db.execute(
                "DELETE FROM events WHERE id NOT IN(SELECT event_id FROM origins)",
                [],
            )?;
        }
        let mut source=old.unwrap_or_else(||json!({"id":id,"files":0,"recognized":0,"ignored":0,"warnings":0,"last_read":null,"last_activity":null}));
        if changed {
            for key in ["files", "recognized", "ignored", "warnings"] {
                source[key] = json!(0);
            }
            source["last_read"] = Value::Null;
            source["last_activity"] = Value::Null;
        }
        source["provider"] = json!(provider);
        source["label"] = json!(args["label"].as_str().unwrap_or(provider));
        source["path"] = json!(canonical);
        source["enabled"] = json!(args["enabled"].as_bool().unwrap_or(true));
        source["exclusions"] = args
            .get("exclusions")
            .cloned()
            .or_else(|| source.get("exclusions").cloned())
            .unwrap_or(json!([]));
        source["status"] = json!(if source["enabled"] == true {
            "pending"
        } else {
            "disabled"
        });
        source["message"] = Value::Null;
        self.put("sources", &id, &source)
    }
    pub fn dispatch(&mut self, command: &str, args: Value) -> Result<Value> {
        match command {
            "snapshot" => self.snapshot(serde_json::from_value(
                args.get("filter").cloned().unwrap_or(json!({})),
            )?),
            "session" => self.detail(
                required(&args, "id")?,
                serde_json::from_value(args.get("filter").cloned().unwrap_or(json!({})))?,
                &args,
            ),
            "compare" => self.compare(&args),
            "export" => self.export(&args),
            "annotate" => {
                let id = required(&args, "id")?;
                if self.get("sessions", id)?.is_none() {
                    bail!("Session is no longer indexed");
                }
                let mut v = self.get("annotations", id)?.unwrap_or(json!({}));
                for key in ["alias", "notes"] {
                    if args.get(key).is_some_and(|x| !x.is_string()) {
                        bail!("{key} must be text");
                    }
                }
                if args.get("tags").is_some_and(|x| {
                    x.as_array()
                        .is_none_or(|a| a.iter().any(|tag| !tag.is_string()))
                }) {
                    bail!("Tags must be an array of text labels");
                }
                if args.get("pinned").is_some_and(|x| !x.is_boolean()) {
                    bail!("Pinned must be a boolean");
                }
                for key in ["alias", "notes", "tags", "pinned"] {
                    if let Some(x) = args.get(key) {
                        v[key] = x.clone();
                    }
                }
                self.put("annotations", id, &v)?;
                Ok(json!({"ok":true}))
            }
            "source_save" => {
                self.save_source(args, false)?;
                self.reconcile()?;
                Ok(json!({"ok":true}))
            }
            "source_remove" => {
                self.db.execute(
                    "DELETE FROM config WHERE id=?1",
                    [format!("bookmark:{}", required(&args, "id")?)],
                )?;
                self.db
                    .execute("DELETE FROM sources WHERE id=?1", [required(&args, "id")?])?;
                Ok(json!({"ok":true}))
            }
            "rescan" => {
                if args["rebuild"] == true {
                    self.db.execute_batch("BEGIN; DELETE FROM files;DELETE FROM events;DELETE FROM origins;DELETE FROM sessions;DELETE FROM markers;DELETE FROM limits; COMMIT;")?;
                    if self.is_demo {
                        self.populate_demo()?;
                    } else {
                        for mut source in self.list("sources")? {
                            for key in ["files", "recognized", "ignored", "warnings"] {
                                source[key] = json!(0);
                            }
                            source["last_read"] = Value::Null;
                            source["last_activity"] = Value::Null;
                            self.put("sources", source["id"].as_str().unwrap_or(""), &source)?;
                        }
                    }
                }
                self.reconcile()?;
                Ok(json!({"ok":true}))
            }
            "settings_save" => {
                let mut s = self.settings()?;
                let updates = args["settings"]
                    .as_object()
                    .context("Settings must be an object")?;
                for (k, v) in updates {
                    if s.get(k).is_some() {
                        s[k] = v.clone();
                    }
                }
                self.validate_settings(&s)?;
                self.put("config", "settings", &s)?;
                Ok(json!({"ok":true}))
            }
            "project_save" => {
                let path = required(&args, "path")?;
                let mut v=self.get("projects",path)?.unwrap_or(json!({"path":path,"name":null,"color":"#d2a56d","notes":"","favorite":false,"aliases":[]}));
                for key in ["name", "color", "notes", "favorite", "aliases"] {
                    if let Some(x) = args.get(key) {
                        v[key] = x.clone();
                    }
                }
                self.put("projects", path, &v)?;
                Ok(json!({"ok":true}))
            }
            "pricing_save" => {
                self.save_price(&args)?;
                Ok(json!({"ok":true}))
            }
            "reprice" => {
                self.reprice()?;
                Ok(json!({"ok":true}))
            }
            "budget_save" => {
                let amount = args["amount"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n > 0.0)
                    .context("Budget amount must be positive")?;
                let threshold = args["threshold"]
                    .as_f64()
                    .filter(|n| n.is_finite() && *n > 0.0 && *n <= 100.0)
                    .context("Alert threshold must be 1–100 percent")?;
                if !["usd", "tokens"].contains(&required(&args, "unit")?)
                    || !["day", "month", "5h", "window"].contains(&required(&args, "period")?)
                {
                    bail!("Unsupported budget unit or period");
                }
                let window_minutes = if args["period"] == "window" {
                    Some(
                        args["window_minutes"]
                            .as_u64()
                            .filter(|n| (1..=525600).contains(n))
                            .context("Custom budget window must be 1–525600 whole minutes")?,
                    )
                } else if args["period"] == "5h" {
                    Some(300)
                } else {
                    None
                };
                let id = args["id"].as_str().map(str::to_owned).unwrap_or_else(|| {
                    hash(&format!(
                        "{}:{}",
                        now(),
                        required(&args, "name").unwrap_or("Budget")
                    ))
                });
                let v = json!({"id":id,"name":required(&args,"name")?,"amount":amount,"threshold":threshold,"unit":args["unit"],"period":args["period"],"window_minutes":window_minutes,"project":args.get("project").cloned().unwrap_or(Value::Null)});
                self.put("budgets", &id, &v)?;
                Ok(json!({"ok":true}))
            }
            "budget_remove" => {
                self.db
                    .execute("DELETE FROM budgets WHERE id=?1", [required(&args, "id")?])?;
                Ok(json!({"ok":true}))
            }
            _ => bail!("Unsupported command: {command}"),
        }
    }
    fn validate_settings(&self, s: &Value) -> Result<()> {
        required(s, "timezone")?
            .parse::<Tz>()
            .context("Use an IANA timezone such as America/Chicago or UTC")?;
        if !["dark", "light", "system"].contains(&required(s, "theme")?) {
            bail!("Invalid theme");
        }
        if !["compact", "comfortable"].contains(&required(s, "density")?) {
            bail!("Invalid density");
        }
        if !(1..=1440).contains(
            &s["inactivity_minutes"]
                .as_u64()
                .context("Inactivity must be whole minutes")?,
        ) {
            bail!("Inactivity must be 1–1440 minutes");
        }
        if !(1..=36500).contains(
            &s["retention_days"]
                .as_u64()
                .context("Retention must be whole days")?,
        ) {
            bail!("Retention must be 1–36500 days");
        }
        for k in [
            "close_to_tray",
            "launch_at_login",
            "notifications",
            "redact_paths",
            "redact_labels",
        ] {
            if !s[k].is_boolean() {
                bail!("{k} must be a boolean");
            }
        }
        if !s["monthly_subscription"].is_null()
            && s["monthly_subscription"]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .is_none()
        {
            bail!("Subscription amount must be nonnegative");
        }
        Ok(())
    }
    pub(crate) fn events(&self) -> Result<Vec<Event>> {
        let enabled: HashSet<String> = self
            .list("sources")?
            .iter()
            .filter(|s| s["enabled"] == true)
            .filter_map(|s| s["id"].as_str().map(str::to_owned))
            .collect();
        let mut origins = self.db.prepare("SELECT event_id,source_id FROM origins")?;
        let rows =
            origins.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut allowed = HashSet::new();
        for r in rows {
            let (id, source) = r?;
            if enabled.contains(&source) {
                allowed.insert(id);
            }
        }
        let mut q = self
            .db
            .prepare("SELECT data,nano FROM events ORDER BY timestamp,id")?;
        let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        let mut out = vec![];
        for row in rows {
            let (s, nano) = row?;
            let mut e: Event = serde_json::from_str(&s)?;
            if allowed.contains(&e.id) {
                e.usage.nano = nano;
                e.usage.cost = nano as f64 / 1e9;
                out.push(e);
            }
        }
        Ok(out)
    }
}
pub(crate) fn required<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .with_context(|| format!("Missing {key}"))
}
