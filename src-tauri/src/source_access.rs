//! App-scoped bookmarks stay in the private index; paths and IPC never confer permission.
#[cfg(feature = "app-store")]
use chip_core::Engine;
#[cfg(feature = "app-store")]
use serde_json::{json, Value};
use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::{Path, PathBuf},
};

#[cfg(feature = "app-store")]
use std::{collections::HashMap, sync::Arc};

unsafe extern "C" {
    #[cfg(feature = "app-store")]
    fn chip_choose_source(directory: bool) -> *mut c_char;
    #[cfg(feature = "app-store")]
    fn chip_resolve(bytes: *const u8, count: usize, stale: *mut bool) -> *mut c_void;
    #[cfg(feature = "app-store")]
    fn chip_refresh(handle: *mut c_void) -> *mut c_char;
    fn chip_url_path(handle: *mut c_void) -> *mut c_char;
    fn chip_start(handle: *mut c_void) -> bool;
    fn chip_stop(handle: *mut c_void);
    fn chip_release(handle: *mut c_void);
    fn chip_free(value: *mut c_char);
    fn chip_save_panel(filename: *const c_char) -> *mut c_void;
    fn chip_restore_panel() -> *mut c_void;
    fn chip_reveal(path: *const c_char);
}
pub fn restore_panel() -> Result<Option<Scope>, String> {
    let handle = unsafe { chip_restore_panel() };
    if handle.is_null() {
        return Ok(None);
    }
    Scope::from_handle(handle, cfg!(feature = "app-store")).map(Some)
}
unsafe fn take_string(raw: *mut c_char) -> Result<String, String> {
    if raw.is_null() {
        return Err("macOS could not return file access information.".into());
    }
    let value = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        chip_free(raw);
    }
    Ok(value)
}
#[cfg(feature = "app-store")]
unsafe fn take_json(raw: *mut c_char) -> Result<Value, String> {
    let value: Value =
        serde_json::from_str(&unsafe { take_string(raw) }?).map_err(|e| e.to_string())?;
    if let Some(error) = value["error"].as_str() {
        return Err(error.into());
    }
    Ok(value)
}

pub struct Scope {
    handle: usize,
    pub path: PathBuf,
    started: bool,
}
// NSURL is immutable. Its retain and access lifecycle is owned by this guard, not a UI object.
impl Drop for Scope {
    fn drop(&mut self) {
        unsafe {
            if self.started {
                chip_stop(self.handle as *mut c_void);
            }
            chip_release(self.handle as *mut c_void);
        }
    }
}
impl Scope {
    fn from_handle(handle: *mut c_void, require_scope: bool) -> Result<Self, String> {
        if handle.is_null() {
            return Err("Permission could not be reopened. Choose the source again.".into());
        }
        let started = unsafe { chip_start(handle) };
        let path = unsafe { take_string(chip_url_path(handle)) };
        let path = PathBuf::from(path.unwrap_or_default());
        let path = path.canonicalize().unwrap_or(path);
        let scope = Self {
            handle: handle as usize,
            path,
            started,
        };
        if require_scope && !started {
            return Err("Read access was revoked. Choose the source again.".into());
        }
        if scope.path.as_os_str().is_empty() {
            return Err("The selected URL has no local path.".into());
        }
        Ok(scope)
    }
    #[cfg(feature = "app-store")]
    fn resolve(bytes: &[u8]) -> Result<(Arc<Self>, Option<Vec<u8>>), String> {
        let mut stale = false;
        let handle = unsafe { chip_resolve(bytes.as_ptr(), bytes.len(), &mut stale) };
        let scope = Arc::new(Self::from_handle(handle, true)?);
        let refreshed = if stale {
            let value = unsafe { take_json(chip_refresh(handle)) }?;
            Some(serde_json::from_value(value["bookmark"].clone()).map_err(|e| e.to_string())?)
        } else {
            None
        };
        Ok((scope, refreshed))
    }
    #[cfg(feature = "app-store")]
    fn readable(&self) -> bool {
        if self.path.is_dir() {
            std::fs::read_dir(&self.path).is_ok()
        } else {
            std::fs::File::open(&self.path).is_ok()
        }
    }
}
#[cfg(feature = "app-store")]
#[derive(Default)]
pub struct SourceAccess {
    active: HashMap<String, Arc<Scope>>,
    // Only one uncommitted picker result per application; replacing/cancelling the dialog
    // cannot leak active scopes. Bookmark resolution occurs on commit, never from a raw path.
    pending: Option<(String, Value)>,
}
#[cfg(feature = "app-store")]
impl SourceAccess {
    pub fn clear(&mut self) {
        self.active.clear();
        self.pending = None;
    }
    pub fn selected(&mut self, value: Value) -> Value {
        if value["cancelled"] == true {
            return Value::Null;
        }
        let token = format!(
            "selection-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        self.pending = Some((token.clone(), value.clone()));
        json!({"path":value["path"],"selection_id":token})
    }
    pub fn prepare_save(&mut self, engine: &mut Engine, args: &mut Value) -> Result<(), String> {
        if let Some(token) = args["selection_id"].as_str() {
            let (_, value) = self
                .pending
                .as_ref()
                .filter(|(id, _)| id == token)
                .ok_or("This selection expired. Choose the source again.")?;
            let bytes: Vec<u8> =
                serde_json::from_value(value["bookmark"].clone()).map_err(|e| e.to_string())?;
            let (scope, refreshed) = Scope::resolve(&bytes)?;
            let id = args["id"].as_str().unwrap_or(token).to_string();
            args["id"] = json!(id);
            args["path"] = json!(scope.path);
            // Validation/save must succeed before persisting a replacement bookmark.
            let mut grants = self.grants();
            grants.insert(id.clone(), scope.path.clone());
            engine.set_source_grants(grants);
            engine
                .save_bookmarked_source(args.clone(), refreshed.as_deref().unwrap_or(&bytes))
                .map_err(|e| e.to_string())?;
            self.active.insert(id, scope);
            self.pending = None;
            engine.reconcile().map_err(|e| e.to_string())?;
            return Ok(());
        }
        // Re-enable a previously disabled root using its bookmark, never the entered path.
        if let Some(id) = args["id"].as_str() {
            if args["enabled"] != false && !self.active.contains_key(id) {
                if let Some(bytes) = engine.source_bookmark(id).map_err(|e| e.to_string())? {
                    let (scope, refreshed) = Scope::resolve(&bytes)?;
                    if let Some(bytes) = refreshed {
                        engine
                            .save_source_bookmark(id, &bytes)
                            .map_err(|e| e.to_string())?;
                    }
                    self.active.insert(id.to_string(), scope);
                    engine.set_source_grants(self.grants());
                }
            }
        }
        // Core validates the existing id/path against the active grant, including edits/toggles.
        engine
            .dispatch("source_save", args.clone())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn refresh(&mut self, engine: &mut Engine) -> Result<Vec<Arc<Scope>>, String> {
        let sources = engine.source_records().map_err(|e| e.to_string())?;
        self.active.retain(|id, scope| {
            sources
                .iter()
                .any(|s| s["id"] == *id && s["enabled"] == true)
                && scope.readable()
        });
        for source in sources.iter().filter(|s| s["enabled"] == true) {
            let Some(id) = source["id"].as_str() else {
                continue;
            };
            if self.active.contains_key(id) {
                continue;
            }
            if let Ok(Some(bytes)) = engine.source_bookmark(id) {
                if let Ok((scope, refreshed)) = Scope::resolve(&bytes) {
                    if !scope.readable() {
                        continue;
                    }
                    if let Some(bytes) = refreshed {
                        engine
                            .save_source_bookmark(id, &bytes)
                            .map_err(|e| e.to_string())?;
                    }
                    // Durable bookmarks follow moved roots. The persisted string is display data.
                    engine
                        .relocate_source(id, &scope.path)
                        .map_err(|e| e.to_string())?;
                    self.active.insert(id.to_string(), scope);
                }
            }
        }
        engine.set_source_grants(self.grants());
        Ok(self.active.values().cloned().collect())
    }
    fn grants(&self) -> HashMap<String, PathBuf> {
        self.active
            .iter()
            .map(|(id, scope)| (id.clone(), scope.path.clone()))
            .collect()
    }
    pub fn allows(&self, path: &Path) -> bool {
        self.active.values().any(|scope| {
            path == scope.path || (scope.path.is_dir() && path.starts_with(&scope.path))
        })
    }
}
#[cfg(feature = "app-store")]
pub fn choose(directory: bool) -> Result<Value, String> {
    unsafe { take_json(chip_choose_source(directory)) }
}
pub fn save_panel(filename: &str) -> Result<Option<Scope>, String> {
    let filename = CString::new(filename).map_err(|e| e.to_string())?;
    let handle = unsafe { chip_save_panel(filename.as_ptr()) };
    if handle.is_null() {
        Ok(None)
    } else {
        Scope::from_handle(handle, false).map(Some)
    }
}
pub fn reveal(path: &Path) -> Result<(), String> {
    let path = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    unsafe {
        chip_reveal(path.as_ptr());
    }
    Ok(())
}
