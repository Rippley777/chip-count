use crate::{Engine, UserError};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// An unopened index is a supported state, never a substitute empty database.
pub struct LocalIndex {
    engine: Option<Engine>,
    path: PathBuf,
    sandboxed: bool,
    error: Option<UserError>,
}
impl LocalIndex {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn open(path: &Path, sandboxed: bool) -> Self {
        let mut index = Self {
            engine: None,
            path: path.into(),
            sandboxed,
            error: None,
        };
        let _ = index.retry();
        index
    }
    pub fn demo() -> Result<Self> {
        Ok(Self {
            engine: Some(Engine::demo()?),
            path: PathBuf::new(),
            sandboxed: false,
            error: None,
        })
    }
    pub fn engine(&mut self) -> Result<&mut Engine> {
        self.engine.as_mut().ok_or_else(|| {
            self.error
                .clone()
                .unwrap_or_else(|| {
                    UserError::new(
                        "index_unavailable",
                        "The index is unavailable.",
                        "Retry opening the index.",
                        self.path.display().to_string(),
                    )
                })
                .into()
        })
    }
    fn retry(&mut self) -> Result<()> {
        if self.engine.is_some() {
            return Ok(());
        }
        let opened = if self.sandboxed {
            Engine::sandboxed(&self.path)
        } else {
            Engine::open(&self.path)
        };
        match opened {
            Ok(mut engine) => {
                // Detect malformed stored JSON before announcing a successful startup.
                if let Err(error) = engine.dispatch("snapshot", json!({})) {
                    let error = UserError::from_error("open_index", &error);
                    self.error = Some(error.clone());
                    return Err(error.into());
                }
                self.engine = Some(engine);
                self.error = None;
                Ok(())
            }
            Err(error) => {
                let mut error = UserError::from_error("open_index", &error);
                error.diagnostics =
                    format!("Index: {}\n{}", self.path.display(), error.diagnostics);
                self.error = Some(error.clone());
                Err(error.into())
            }
        }
    }
    pub fn dispatch(&mut self, command: &str, args: Value) -> Result<Value> {
        match command {
            "index_retry" => {
                let backup = if self.engine.is_none() && self.path.exists() {
                    Some(preserve_files(&self.path)?)
                } else {
                    None
                };
                self.retry()?;
                Ok(json!({"ok": true, "backup":backup}))
            }
            "index_preserve" => {
                let backup = if let Some(engine) = &self.engine {
                    engine.preserve("recovery")?
                } else {
                    preserve_files(&self.path)?
                };
                Ok(json!({"ok":true, "backup":backup}))
            }
            _ => self.engine()?.dispatch(command, args),
        }
    }
    /// Only restore an unopened index, using a validated, standalone snapshot.
    /// The native host owns selection and read permission; IPC never accepts a path.
    pub fn restore(&mut self, selected: &Path) -> Result<PathBuf> {
        anyhow::ensure!(
            self.engine.is_none(),
            "The index is already available; restore is only offered during startup recovery"
        );
        let source = rusqlite::Connection::open_with_flags(
            selected,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        let integrity: String = source.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        anyhow::ensure!(
            integrity == "ok",
            "The selected backup failed its integrity check"
        );
        let version: i64 = source
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .context("Select a Chip Count backup; this SQLite file has no Chip Count schema")?;
        anyhow::ensure!(
            version == 1,
            "Select a compatible Chip Count backup (schema version 1)"
        );
        for table in [
            "annotations",
            "prices",
            "budgets",
            "projects",
            "config",
            "sessions",
            "sources",
        ] {
            let exists: bool = source.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |r| r.get(0),
            )?;
            anyhow::ensure!(
                exists,
                "The selected backup is missing {table}; the original index has been kept"
            );
        }
        let parent = self
            .path
            .parent()
            .context("Index directory is unavailable")?;
        let staged = parent.join(format!(
            "restore-{}.sqlite",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        source.execute("VACUUM INTO ?1", [staged.to_string_lossy().as_ref()])?;
        drop(source);
        // Validate schema and local metadata on the staged file before touching originals.
        let mut candidate = Engine::isolated(&staged)?;
        candidate.dispatch("snapshot", json!({}))?;
        candidate
            .db
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        drop(candidate);
        let backup = preserve_files(&self.path)?;
        // No open engine or watcher can write these files. Retire sidecars alongside
        // their recovery copies, so stale WAL records cannot enter the restored DB.
        let mut retired = Vec::new();
        for suffix in ["-wal", "-shm", "-journal"] {
            let old = PathBuf::from(format!("{}{suffix}", self.path.display()));
            if old.exists() {
                let target = backup.join(format!("retired{suffix}"));
                if let Err(error) = std::fs::rename(&old, &target) {
                    for (old, target) in retired.iter().rev() {
                        let _ = std::fs::rename(target, old);
                    }
                    return Err(error.into());
                }
                retired.push((old, target));
            }
        }
        if let Err(error) = std::fs::rename(&staged, &self.path) {
            for (old, target) in retired.iter().rev() {
                let _ = std::fs::rename(target, old);
            }
            return Err(error.into());
        }
        self.retry()?;
        Ok(backup)
    }
}

/// Corrupt files cannot use SQLite backup. Copy the DB and every sidecar together,
/// without renaming, deleting, or opening the original for writes.
pub fn preserve_files(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("Index directory is unavailable")?;
    let backup = parent.join(format!(
        "recovery-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir(&backup)
        .context("Could not create recovery directory; original index has been kept")?;
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let db_name = path
            .file_name()
            .context("Index filename is unavailable")?
            .to_string_lossy();
        if entry.path().is_file()
            && (name == db_name
                || name == format!("{db_name}-wal")
                || name == format!("{db_name}-shm")
                || name == format!("{db_name}-journal")
                || name == "notified-alerts.json")
        {
            std::fs::copy(entry.path(), backup.join(entry.file_name()))?;
            std::fs::File::open(backup.join(entry.file_name()))?.sync_all()?;
        }
    }
    Ok(backup)
}
