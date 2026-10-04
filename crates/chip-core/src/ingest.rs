use crate::{model::*, parser, Engine};
use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use globset::{Glob, GlobSetBuilder};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
const MAX_FILES: usize = 50_000;
const MAX_ENTRIES: usize = 100_000;
const MAX_LINES: usize = 1_000;
const MAX_LINE_BYTES: u64 = 8 * 1024 * 1024;
impl Engine {
    pub fn reconcile(&mut self) -> Result<()> {
        self.reconcile_with_budget(std::time::Duration::from_millis(750))
    }
    fn reconcile_with_budget(&mut self, pass_budget: std::time::Duration) -> Result<()> {
        self.reconcile_with_limits(pass_budget, MAX_ENTRIES)
    }
    fn reconcile_with_limits(
        &mut self,
        pass_budget: std::time::Duration,
        entry_limit: usize,
    ) -> Result<()> {
        if self.is_demo {
            return Ok(());
        }
        let pass_started = std::time::Instant::now();
        let mut sources = vec![];
        for mut source in self.list("sources")? {
            if source["enabled"] != true {
                source["status"] = json!("disabled");
                self.put("sources", source["id"].as_str().unwrap_or(""), &source)?;
            } else if !self.source_authorized(&source) {
                source["status"] = json!("unavailable");
                source["message"] = json!("Access is unavailable. Choose this folder or JSONL file again in Configure source to grant read-only access.");
                self.put("sources", source["id"].as_str().unwrap_or(""), &source)?;
            } else {
                sources.push(source);
            }
        }
        // Checkpointed rotations prevent a large or busy first root from monopolizing
        // every pass. Missing/deleted roots and files do not invalidate the cursor.
        if let Some(cursor) = self
            .get("config", "reconcile_source_cursor")?
            .and_then(|v| v.as_str().map(str::to_owned))
        {
            let next = sources
                .iter()
                .position(|s| s["id"].as_str().unwrap_or("") > cursor.as_str())
                .unwrap_or(0);
            sources.rotate_left(next);
        }
        for (source_index, mut source) in sources.into_iter().enumerate() {
            if source_index > 0 && pass_started.elapsed() >= pass_budget {
                break;
            }
            let root = PathBuf::from(source["path"].as_str().unwrap_or(""));
            let id = source["id"].as_str().unwrap_or("").to_owned();
            self.put("config", "reconcile_source_cursor", &json!(id))?;
            if !root.exists() {
                source["status"] = json!("missing");
                source["message"] =
                    json!("Folder or file is unavailable. Reconnect it or choose another path.");
                self.put("sources", &id, &source)?;
                continue;
            }
            let mut builder = GlobSetBuilder::new();
            if let Some(patterns) = source["exclusions"].as_array() {
                for p in patterns {
                    if let Some(p) = p.as_str() {
                        builder.add(Glob::new(p)?);
                    }
                }
            }
            let exclusions = builder.build()?;
            let mut paths = vec![];
            let mut inaccessible = 0;
            let mut traversal_limit = None;
            if root.is_file() {
                paths.push(root.clone());
            } else {
                let mut walk = WalkDir::new(&root)
                    .max_depth(12)
                    .follow_links(false)
                    .into_iter();
                let mut visited = 0;
                while visited < entry_limit {
                    let Some(entry) = walk.next() else {
                        break;
                    };
                    visited += 1;
                    // Count every encountered entry, including excluded paths and
                    // failures. Prune excluded directories before descending them.
                    if let Ok(entry) = &entry {
                        if exclusions
                            .is_match(entry.path().strip_prefix(&root).unwrap_or(entry.path()))
                        {
                            if entry.file_type().is_dir() {
                                walk.skip_current_dir();
                            }
                            continue;
                        }
                    }
                    match entry {
                        Ok(e)
                            if e.file_type().is_file()
                                && e.path().extension().and_then(|x| x.to_str())
                                    == Some("jsonl") =>
                        {
                            paths.push(e.into_path());
                            if paths.len() >= MAX_FILES {
                                traversal_limit = Some(format!("Traversal reached the {MAX_FILES}-JSONL-file safety limit; choose narrower source roots for full coverage."));
                                break;
                            }
                        }
                        Err(_) => inaccessible += 1,
                        _ => {}
                    }
                }
                if visited >= entry_limit {
                    traversal_limit = Some(format!("Traversal reached the {entry_limit}-entry safety limit, including directories and unsupported files; some paths may be unread. Choose narrower source roots or exclude unrelated directories for full coverage."));
                }
            }
            paths.sort();
            let cursor_key = format!("reconcile_file_cursor:{id}");
            if let Some(cursor) = self
                .get("config", &cursor_key)?
                .and_then(|v| v.as_str().map(PathBuf::from))
            {
                let next = paths.iter().position(|p| p > &cursor).unwrap_or(0);
                paths.rotate_left(next);
            }
            source["files"] = json!(paths.len());
            let mut partial = false;
            let mut checked = source["files"] == 0 && inaccessible == 0;
            let mut attempted = 0;
            let mut last_path = None;
            for path in paths {
                // Traversal can itself exceed the soft budget. Always attempt one
                // file, then yield; the next pass resumes after that file.
                if attempted > 0 && pass_started.elapsed() >= pass_budget {
                    partial = true;
                    break;
                }
                if exclusions.is_match(path.strip_prefix(&root).unwrap_or(&path)) {
                    continue;
                }
                attempted += 1;
                last_path = Some(path.clone());
                match self.ingest_file(&source, &path) {
                    Ok((more, recognized, ignored, warnings, activity, _changed)) => {
                        partial |= more;
                        checked = true;
                        for (k, n) in [
                            ("recognized", recognized),
                            ("ignored", ignored),
                            ("warnings", warnings),
                        ] {
                            source[k] = json!(source[k].as_u64().unwrap_or(0) + n);
                        }
                        if let Some(at) = activity {
                            if source["last_activity"].as_str().unwrap_or("") < at.as_str() {
                                source["last_activity"] = json!(at);
                            }
                        }
                    }
                    Err(_) => {
                        inaccessible += 1;
                        source["warnings"] = json!(source["warnings"].as_u64().unwrap_or(0) + 1);
                    }
                }
            }
            if let Some(path) = last_path {
                self.put("config", &cursor_key, &json!(path))?;
            }
            if checked {
                source["last_read"] = json!(now());
            }
            source["status"] = json!(if inaccessible > 0 || traversal_limit.is_some() {
                "partial"
            } else if partial {
                "indexing"
            } else if source["files"] == 0 {
                "empty"
            } else if source["recognized"] == 0 {
                "unsupported"
            } else {
                "healthy"
            });
            source["message"] = if let Some(message) = traversal_limit {
                json!(message)
            } else if inaccessible > 0 {
                json!(format!(
                    "{inaccessible} file or folder reads failed; check access permissions."
                ))
            } else if partial {
                json!("Indexing in bounded batches; remaining complete records will be read on the next pass.")
            } else {
                Value::Null
            };
            self.put("sources", &id, &source)?;
        }
        // Retention removes derived usage metadata only. Checkpoints prevent old records being re-added on every poll.
        let days = self.settings()?["retention_days"].as_i64().unwrap_or(365);
        let cutoff = (Utc::now() - Duration::days(days))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        self.db
            .execute("DELETE FROM events WHERE timestamp < ?1", [cutoff])?;
        self.db.execute(
            "DELETE FROM origins WHERE event_id NOT IN(SELECT id FROM events)",
            [],
        )?;
        self.put("config", "indexed_at", &json!(now()))?;
        Ok(())
    }
    fn ingest_file(
        &mut self,
        source: &Value,
        path: &Path,
    ) -> Result<(bool, u64, u64, u64, Option<String>, bool)> {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let path_s = path.to_string_lossy().to_string();
        let source_id = source["id"].as_str().unwrap_or("");
        // Source transcripts are always opened read-only, including rebuilds.
        let mut file = File::open(&path).context("Cannot open source file")?;
        let metadata = file.metadata()?;
        let length = metadata.len();
        let old:Option<(u64,u64,u64,String,String)>=self.db.query_row("SELECT offset,line,length,fingerprint,state FROM files WHERE path=?1 AND source_id=?2",params![path_s,source_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let mut reset = false;
        let (mut offset, mut line, mut state) = (0, 0, ParseState::default());
        if let Some((saved_offset, saved_line, _, signature, saved_state)) = &old {
            let probe_len = signature
                .split(':')
                .next()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let actual = fingerprint(&mut file, probe_len, *saved_offset)?;
            reset = length < *saved_offset || signature != &actual;
            if !reset {
                offset = *saved_offset;
                line = *saved_line;
                state = serde_json::from_str(saved_state)?;
            }
        }
        if offset == length && !reset && old.is_some() {
            return Ok((false, 0, 0, 0, None, false));
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let (mut recognized, mut ignored, mut warnings) = (0, 0, 0);
        let mut complete = 0;
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            if reset {
                self.db.execute(
                    "DELETE FROM origins WHERE path=?1 AND source_id=?2",
                    params![path_s, source_id],
                )?;
                self.db.execute(
                    "DELETE FROM events WHERE id NOT IN(SELECT event_id FROM origins)",
                    [],
                )?;
                warnings += 1;
            }
            while complete < MAX_LINES {
                let mut bytes = vec![];
                let read = reader
                    .by_ref()
                    .take(MAX_LINE_BYTES + 1)
                    .read_until(b'\n', &mut bytes)?;
                if read == 0 {
                    break;
                }
                if read as u64 > MAX_LINE_BYTES {
                    // Oversized transcript lines are discarded without retaining their text.
                    let mut terminated = bytes.ends_with(b"\n");
                    let mut consumed = read as u64;
                    while !terminated {
                        let buf = reader.fill_buf()?;
                        if buf.is_empty() {
                            break;
                        }
                        let pos = buf.iter().position(|x| *x == b'\n');
                        let used = pos.map(|i| i + 1).unwrap_or(buf.len());
                        terminated = pos.is_some();
                        reader.consume(used);
                        consumed += used as u64;
                    }
                    if !terminated {
                        break;
                    }
                    offset += consumed;
                    line += 1;
                    complete += 1;
                    warnings += 1;
                    continue;
                }
                if !bytes.ends_with(b"\n") {
                    break;
                } // Durable offset remains before incomplete trailing record.
                offset += read as u64;
                line += 1;
                complete += 1;
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(v) => {
                        let parsed = parser::parse(
                            source["provider"].as_str().unwrap_or(""),
                            &v,
                            &mut state,
                            &path_s,
                            line,
                        );
                        if parsed.recognized {
                            recognized += 1;
                            if !state.session.first_at.is_empty() {
                                self.merge_session(&state.session)?;
                            }
                        } else {
                            ignored += 1;
                        }
                        if parsed.warning.is_some() {
                            warnings += 1;
                        }
                        for mut event in parsed.events {
                            warnings += event.warnings.len() as u64;
                            self.insert_event(&mut event, source_id, &path_s)?;
                        }
                        if let Some(marker) = parsed.marker {
                            let id = hash(&format!("{}:{}", state.session.id, marker));
                            self.db.execute("INSERT OR IGNORE INTO markers(id,session_id,data) VALUES(?1,?2,?3)",params![id,state.session.id,marker.to_string()])?;
                        }
                        for mut limit in parsed.limits {
                            let scope = format!(
                                "{} / {}",
                                source["label"].as_str().unwrap_or("Source"),
                                limit["scope"].as_str().unwrap_or("reported")
                            );
                            limit["scope"] = json!(scope);
                            let id = hash(&format!("{}:{}", source_id, scope));
                            self.put("limits", &id, &limit)?;
                        }
                    }
                    Err(_) => warnings += 1,
                }
            }
            if !state.session.id.is_empty() && !state.session.first_at.is_empty() {
                self.merge_session(&state.session)?;
            }
            let signature = fingerprint(reader.get_mut(), length.min(1024), offset)?;
            self.db.execute("INSERT INTO files(path,source_id,offset,line,length,fingerprint,state) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(path,source_id) DO UPDATE SET offset=excluded.offset,line=excluded.line,length=excluded.length,fingerprint=excluded.fingerprint,state=excluded.state",params![path_s,source_id,offset,line,length,signature,serde_json::to_string(&state)?])?;
            Ok(())
        })();
        if let Err(error) = result {
            self.db.execute_batch("ROLLBACK")?;
            return Err(error);
        }
        self.db.execute_batch("COMMIT")?;
        Ok((
            complete >= MAX_LINES && offset < length,
            recognized,
            ignored,
            warnings,
            if state.session.last_at.is_empty() {
                None
            } else {
                Some(state.session.last_at)
            },
            true,
        ))
    }
    pub(crate) fn merge_session(&self, new: &SessionMeta) -> Result<()> {
        let mut value = new.clone();
        if let Some(old) = self.get("sessions", &new.id)? {
            let old: SessionMeta = serde_json::from_value(old)?;
            if !old.first_at.is_empty()
                && (value.first_at.is_empty() || old.first_at < value.first_at)
            {
                value.first_at = old.first_at;
            }
            if old.last_at > value.last_at {
                value.last_at = old.last_at;
                value.completed = old.completed;
            }
            if value.project_path.is_empty() {
                value.project_path = old.project_path;
            }
            if value.parent_id.is_none() {
                value.parent_id = old.parent_id;
            }
        }
        self.put("sessions", &value.id, &serde_json::to_value(&value)?)
    }
    pub(crate) fn insert_event(
        &self,
        event: &mut Event,
        source_id: &str,
        path: &str,
    ) -> Result<()> {
        let old: Option<(String, i64)> = self
            .db
            .query_row(
                "SELECT data,nano FROM events WHERE id=?1",
                [&event.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let previous = old
            .as_ref()
            .map(|(s, _)| serde_json::from_str::<Event>(s))
            .transpose()?;
        let mut should_update = true;
        if let Some(previous) = &previous {
            let previous_final = previous.kind == "response-final";
            let candidate_final = event.kind == "response-final";
            should_update = (!previous_final || candidate_final)
                && (event.timestamp >= previous.timestamp || candidate_final && !previous_final);
            let is_child = |id: &str| -> Result<bool> {
                Ok(id.contains("/agent:")
                    || self
                        .get("sessions", id)?
                        .is_some_and(|m| m["parent_id"].is_string()))
            };
            let candidate_child = is_child(&event.session_id)?;
            let previous_child = is_child(&previous.session_id)?;
            // A replay is provenance for the original request, not a new child's own usage.
            if !previous_child && candidate_child {
                event.session_id = previous.session_id.clone();
            } else if previous_child && !candidate_child {
                let canonical_owner = event.session_id.clone();
                if !should_update {
                    *event = previous.clone();
                    event.session_id = canonical_owner;
                    should_update = true;
                }
            }
            event.pricing_version = if previous.model == event.model {
                previous.pricing_version.clone()
            } else {
                event.warnings.push(
                    "Model metadata was corrected; estimate now uses the identified model's rate"
                        .into(),
                );
                None
            };
            event.ingested_at = previous.ingested_at.clone();
            let merge = |field: &str, new: &mut u64, old: u64, unknown: &mut Vec<String>| {
                if unknown.iter().any(|s| s == field)
                    && !previous.usage.unknown_fields.iter().any(|s| s == field)
                {
                    *new = old;
                    unknown.retain(|s| s != field);
                } else if event.kind == "response-snapshot"
                    && previous.kind == "response-snapshot"
                    && !previous.usage.unknown_fields.iter().any(|s| s == field)
                {
                    *new = (*new).max(old);
                }
            };
            let u = &mut event.usage;
            merge(
                "input",
                &mut u.input,
                previous.usage.input,
                &mut u.unknown_fields,
            );
            merge(
                "output",
                &mut u.output,
                previous.usage.output,
                &mut u.unknown_fields,
            );
            merge(
                "cache_read",
                &mut u.cache_read,
                previous.usage.cache_read,
                &mut u.unknown_fields,
            );
            merge(
                "cache_write",
                &mut u.cache_write,
                previous.usage.cache_write,
                &mut u.unknown_fields,
            );
            merge(
                "reasoning",
                &mut u.reasoning,
                previous.usage.reasoning,
                &mut u.unknown_fields,
            );
            u.total = u
                .input
                .saturating_add(u.output)
                .saturating_add(u.cache_read)
                .saturating_add(u.cache_write);
        }
        if should_update {
            self.apply_price(event)?;
            self.db.execute("INSERT INTO events(id,session_id,timestamp,model,scope,data,nano) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET session_id=excluded.session_id,timestamp=excluded.timestamp,model=excluded.model,scope=excluded.scope,data=excluded.data,nano=excluded.nano",params![event.id,event.session_id,event.timestamp,event.model,event.scope,serde_json::to_string(event)?,event.usage.nano])?;
        }
        self.db.execute(
            "INSERT OR IGNORE INTO origins(event_id,path,source_id) VALUES(?1,?2,?3)",
            params![event.id, path, source_id],
        )?;
        Ok(())
    }
}
fn fingerprint(file: &mut File, n: u64, offset: u64) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut prefix = vec![];
    file.take(n).read_to_end(&mut prefix)?;
    let tail_start = offset.saturating_sub(1024);
    file.seek(SeekFrom::Start(tail_start))?;
    let mut tail = vec![];
    file.take(offset - tail_start).read_to_end(&mut tail)?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata()?;
        format!("{}:{}", m.dev(), m.ino())
    };
    #[cfg(not(unix))]
    let identity = String::from("0:0");
    Ok(format!(
        "{}:{}:{}:{}:{}",
        n,
        identity,
        hash(&String::from_utf8_lossy(&prefix)),
        offset,
        hash(&String::from_utf8_lossy(&tail))
    ))
}

#[cfg(test)]
mod reconciliation_tests {
    use super::*;
    use std::time::Duration as StdDuration;

    fn response(request: &str) -> Value {
        json!({"type":"assistant","sessionId":"fairness","requestId":request,"timestamp":now(),"message":{"id":request,"model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}})
    }
    fn add(engine: &Engine, id: &str, path: &Path) {
        engine
            .save_source(
                json!({"id":id,"provider":"claude","path":path,"label":id,"enabled":true}),
                false,
            )
            .unwrap();
    }
    fn total(engine: &mut Engine) -> u64 {
        engine.dispatch("snapshot", json!({})).unwrap()["totals"]["total"]
            .as_u64()
            .unwrap()
    }
    #[test]
    fn exhausted_budget_still_progresses_fairly_across_roots_and_files_after_restart() {
        let temp = tempfile::TempDir::new().unwrap();
        let db = temp.path().join("index.sqlite");
        let mut engine = Engine::isolated(&db).unwrap();
        for root in ["alpha", "beta"] {
            let path = temp.path().join(root);
            std::fs::create_dir(&path).unwrap();
            for file in ["a", "b"] {
                std::fs::write(
                    path.join(format!("{file}.jsonl")),
                    format!("{}\n", response(&format!("{root}-{file}"))),
                )
                .unwrap();
            }
            add(&engine, root, &path);
        }
        // Zero budget deterministically models a traversal that already consumed
        // the deadline; useful work must still occur, without waiting in tests.
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(total(&mut engine), 1);
        assert_eq!(
            engine.get("config", "reconcile_source_cursor").unwrap(),
            Some(json!("alpha"))
        );
        drop(engine);
        let mut engine = Engine::isolated(&db).unwrap();
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(total(&mut engine), 2);
        assert_eq!(
            engine.get("config", "reconcile_source_cursor").unwrap(),
            Some(json!("beta"))
        );
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(total(&mut engine), 3);
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(total(&mut engine), 4);
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(
            total(&mut engine),
            4,
            "Unchanged revisits remain idempotent"
        );
    }
    #[test]
    fn unfinished_large_file_cannot_starve_later_file_and_is_resumed() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut engine = Engine::isolated(&temp.path().join("index.sqlite")).unwrap();
        let root = temp.path().join("logs");
        std::fs::create_dir(&root).unwrap();
        let records = (0..=MAX_LINES)
            .map(|i| format!("{}\n", response(&format!("long-{i}"))))
            .collect::<String>();
        std::fs::write(root.join("a-large.jsonl"), records).unwrap();
        std::fs::write(
            root.join("b-small.jsonl"),
            format!("{}\n", response("small")),
        )
        .unwrap();
        add(&engine, "source", &root);
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(total(&mut engine), MAX_LINES as u64);
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(total(&mut engine), MAX_LINES as u64 + 1);
        assert_eq!(
            engine
                .events()
                .unwrap()
                .iter()
                .filter(|e| e.request_id.as_deref() == Some("small"))
                .count(),
            1
        );
        engine.reconcile_with_budget(StdDuration::ZERO).unwrap();
        assert_eq!(total(&mut engine), MAX_LINES as u64 + 2);
    }
    #[test]
    fn traversal_caps_unsupported_and_excluded_entries_with_partial_diagnostic() {
        for exclude_text_files in [false, true] {
            let temp = tempfile::TempDir::new().unwrap();
            let mut engine = Engine::isolated(&temp.path().join("index.sqlite")).unwrap();
            let root = temp.path().join("logs");
            std::fs::create_dir(&root).unwrap();
            for index in 0..8 {
                std::fs::write(root.join(format!("unsupported-{index}.txt")), "not a log").unwrap();
            }
            engine.save_source(json!({"id":"bounded","provider":"claude","path":root,"label":"Bounded","enabled":true,"exclusions":if exclude_text_files{vec!["*.txt"]}else{vec![]}}),false).unwrap();
            engine
                .reconcile_with_limits(StdDuration::from_secs(1), 3)
                .unwrap();
            let source = engine.get("sources", "bounded").unwrap().unwrap();
            assert_eq!(source["files"], 0);
            assert_eq!(source["status"], "partial");
            let diagnostic = source["message"].as_str().unwrap();
            assert!(diagnostic.contains("3-entry safety limit"));
            assert!(diagnostic.contains("narrower source roots"));
            assert_eq!(total(&mut engine), 0);
        }
    }
}
