use serde::Serialize;

/// Stable IPC error contract. Diagnostics contain context, never transcript contents.
#[derive(Clone, Debug, Serialize)]
pub struct UserError {
    pub category: &'static str,
    pub message: String,
    pub next_steps: String,
    pub diagnostics: String,
}
impl UserError {
    pub fn new(
        category: &'static str,
        message: impl Into<String>,
        next_steps: impl Into<String>,
        diagnostics: impl Into<String>,
    ) -> Self {
        Self {
            category,
            message: message.into(),
            next_steps: next_steps.into(),
            diagnostics: diagnostics.into(),
        }
    }
    pub fn from_error(operation: &str, error: &anyhow::Error) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(rusqlite::Error::SqliteFailure(code, _)) =
            error.downcast_ref::<rusqlite::Error>()
        {
            use rusqlite::ErrorCode::*;
            let (category, message, steps) = match code.code {
                DatabaseCorrupt | NotADatabase => ("index_corrupt", "The local index could not be read.", "Preserve a recovery copy, then restore a known-good backup. Your original index and notes are kept; Chip Count will not rebuild automatically."),
                DatabaseBusy | DatabaseLocked => ("index_unavailable", "The local index is busy.", "Close other Chip Count instances and retry."),
                _ => ("save_failed", "The local index could not complete the change.", "Check available disk space and write access, then retry. Keep this window open to retain your draft."),
            };
            return Self::new(category, message, steps, format!("{operation}: {error:#}"));
        }
        if error.downcast_ref::<std::io::Error>().is_some() {
            return Self::new("access_unavailable", "The file or folder is unavailable.", "Check permissions and disk space, reconnect the volume, or select the path again; then retry.", format!("{operation}: {error:#}"));
        }
        if error.downcast_ref::<serde_json::Error>().is_some()
            && matches!(operation, "open_index" | "snapshot")
        {
            return Self::new("index_corrupt", "Stored index metadata could not be read.", "Preserve a recovery copy and restore a known-good backup. Notes will not be discarded automatically.", format!("{operation}: {error:#}"));
        }
        if operation.ends_with("_save") || matches!(operation, "annotate" | "compare" | "export") {
            return Self::new(
                "invalid_input",
                error.to_string(),
                "Correct the value and try again. Your previous saved values are retained.",
                format!("{operation}: {error:#}"),
            );
        }
        Self::new("index_unavailable", "The local index is unavailable.", "Retry after checking disk access. Open source diagnostics if a source cannot be read. Preserve a recovery copy before restoring an index.", format!("{operation}: {error:#}"))
    }
    pub fn native(operation: &str, message: String) -> Self {
        let (category, steps) = if operation == "save_export" {
            ("save_failed", "Choose a writable destination outside source folders, check disk space, and retry. The report remains available.")
        } else {
            (
                "access_unavailable",
                "Retry, or select the file or folder again. Check access permissions in Sources.",
            )
        };
        Self::new(
            category,
            message.clone(),
            steps,
            format!("{operation}: {message}"),
        )
    }
}
impl std::fmt::Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.message, self.next_steps)
    }
}
impl std::error::Error for UserError {}
