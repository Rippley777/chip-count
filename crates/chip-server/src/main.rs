//! Development-only loopback bridge. Production embeds the same Engine in Tauri.
use anyhow::{bail, Context, Result};
use chip_core::Engine;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const ADDRESS: &str = "127.0.0.1:4319";
const BODY_LIMIT: u64 = 4 * 1024 * 1024;
#[derive(Deserialize)]
struct Dispatch {
    command: String,
    #[serde(default = "empty_object")]
    args: Value,
    #[serde(default)]
    demo: bool,
}
fn empty_object() -> Value {
    json!({})
}
struct State {
    real: Arc<Mutex<Engine>>,
    demo: Arc<Mutex<Engine>>,
    paused: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
}

fn data_path() -> Result<PathBuf> {
    let mut args = std::env::args().skip(1);
    let mut db = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = Some(PathBuf::from(args.next().context("--db requires a path")?)),
            "--help" | "-h" => {
                println!("chip-server [--db PATH]\nDevelopment API at http://{ADDRESS}\nCHIP_COUNT_DATA_DIR overrides the default application data directory.");
                std::process::exit(0);
            }
            _ => bail!("Unknown argument {arg}. Use --help."),
        }
    }
    let path = db.unwrap_or_else(|| {
        std::env::var_os("CHIP_COUNT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::data_local_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("labs.rippley.chip-count")
            })
            .join("chip-count.sqlite")
    });
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    Ok(path)
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}
fn origin_allowed(origin: &str) -> bool {
    matches!(
        origin,
        "http://127.0.0.1:1431"
            | "http://localhost:1431"
            | "http://127.0.0.1:4319"
            | "http://localhost:4319"
    )
}
fn request_allowed(request: &Request) -> bool {
    let host = header(request, "host").unwrap_or_default();
    matches!(
        host,
        "127.0.0.1:4319" | "localhost:4319" | "127.0.0.1:1431" | "localhost:1431"
    ) && header(request, "origin").is_none_or(origin_allowed)
        && header(request, "sec-fetch-site").is_none_or(|site| site != "cross-site")
}
fn respond(request: Request, status: u16, value: Value) {
    let mut response = Response::from_string(value.to_string())
        .with_status_code(StatusCode(status))
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap());
    if let Some(origin) = header(&request, "origin").filter(|v| origin_allowed(v)) {
        response.add_header(Header::from_bytes("Access-Control-Allow-Origin", origin).unwrap());
        response.add_header(Header::from_bytes("Vary", "Origin").unwrap());
        response.add_header(
            Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap(),
        );
        response.add_header(
            Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap(),
        );
    }
    let _ = request.respond(response);
}
fn read_json(request: &mut Request) -> Result<Value> {
    if !header(request, "content-type").is_some_and(|v| {
        v.split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("application/json")
    }) {
        bail!("Content-Type must be application/json");
    }
    if request.body_length().is_some_and(|n| n as u64 > BODY_LIMIT) {
        bail!("Request exceeds the 4 MB limit");
    }
    let mut body = Vec::new();
    request
        .as_reader()
        .take(BODY_LIMIT + 1)
        .read_to_end(&mut body)?;
    if body.len() as u64 > BODY_LIMIT {
        bail!("Request exceeds the 4 MB limit");
    }
    serde_json::from_slice(&body).context("Request body must contain valid JSON")
}

fn watch_roots(
    watcher: &mut Option<RecommendedWatcher>,
    current: &mut BTreeSet<PathBuf>,
    roots: Vec<PathBuf>,
) {
    let next: BTreeSet<_> = roots
        .into_iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect();
    if let Some(watcher) = watcher {
        for root in current.difference(&next) {
            let _ = watcher.unwatch(root);
        }
        for root in next.difference(current) {
            let _ = watcher.watch(root, RecursiveMode::Recursive);
        }
    }
    *current = next;
}
fn monitor(state: Arc<State>, receiver: mpsc::Receiver<()>) {
    std::thread::Builder::new()
        .name("chip-indexer".into())
        .spawn(move || {
            let wake = state.wake.clone();
            let mut watcher =
                notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                    if event.is_ok_and(|event| {
                        event.paths.iter().any(|path| {
                            path.extension().is_some_and(|ext| ext == "jsonl") || path.is_dir()
                        })
                    }) {
                        let _ = wake.try_send(());
                    }
                })
                .ok();
            let mut roots = BTreeSet::new();
            loop {
                if !state.paused.load(Ordering::Relaxed) {
                    if let Ok(mut engine) = state.real.lock() {
                        if engine.reconcile().is_err() {
                            eprintln!(
                                "Index reconciliation failed. Check Sources for diagnostics."
                            );
                        }
                        watch_roots(&mut watcher, &mut roots, engine.watch_roots());
                    }
                }
                if receiver.recv_timeout(Duration::from_secs(3)).is_ok() {
                    std::thread::sleep(Duration::from_millis(150));
                }
                while receiver.try_recv().is_ok() {}
            }
        })
        .expect("could not start index worker");
}

fn handle(mut request: Request, state: &State) {
    if !request_allowed(&request) {
        respond(
            request,
            403,
            json!({"error":"Only requests from the local Chip Count development UI are allowed."}),
        );
        return;
    }
    if request.method() == &Method::Options {
        respond(request, 200, json!({"ok":true}));
        return;
    }
    if request.method() == &Method::Get && request.url() == "/api/health" {
        respond(
            request,
            200,
            json!({"ok":true,"service":"chip-count","paused":state.paused.load(Ordering::Relaxed)}),
        );
        return;
    }
    if request.method() != &Method::Post
        || !matches!(request.url(), "/api/dispatch" | "/api/monitoring")
    {
        respond(request, 404, json!({"error":"Unknown local API route."}));
        return;
    }
    let body = match read_json(&mut request) {
        Ok(v) => v,
        Err(e) => {
            respond(request, 400, json!({"error":e.to_string()}));
            return;
        }
    };
    if request.url() == "/api/monitoring" {
        let Some(paused) = body["paused"].as_bool() else {
            respond(request, 400, json!({"error":"paused must be a boolean"}));
            return;
        };
        state.paused.store(paused, Ordering::Relaxed);
        let _ = state.wake.try_send(());
        respond(request, 200, json!({"paused":paused}));
        return;
    }
    let dispatch: Dispatch = match serde_json::from_value(body) {
        Ok(v) => v,
        Err(_) => {
            respond(
                request,
                400,
                json!({"error":"Expected {command, args, demo}."}),
            );
            return;
        }
    };
    let engine = if dispatch.demo {
        &state.demo
    } else {
        &state.real
    };
    let result = match engine.lock() {
        Ok(mut engine) => engine
            .dispatch(&dispatch.command, dispatch.args)
            .map_err(|e| e.to_string()),
        Err(_) => Err("The local index is unavailable; restart the development server.".into()),
    };
    match result {
        Ok(value) => {
            if !dispatch.demo
                && !matches!(
                    dispatch.command.as_str(),
                    "snapshot" | "session" | "compare" | "export"
                )
            {
                let _ = state.wake.try_send(());
            }
            respond(request, 200, value);
        }
        Err(error) => respond(request, 400, json!({"error":error})),
    }
}

fn main() -> Result<()> {
    let db = data_path()?;
    // Bind before opening the database to avoid two servers indexing the same file.
    let server =
        Server::http(ADDRESS).map_err(|e| anyhow::anyhow!("Could not bind {ADDRESS}: {e}"))?;
    let real = Arc::new(Mutex::new(Engine::open(&db)?));
    let demo = Arc::new(Mutex::new(Engine::demo()?));
    let (wake, receiver) = mpsc::sync_channel(1);
    let state = Arc::new(State {
        real,
        demo,
        paused: Arc::new(AtomicBool::new(false)),
        wake,
    });
    monitor(state.clone(), receiver);
    println!("Chip Count Rust development API: http://{ADDRESS}");
    println!("Local database: {}", db.display());
    for request in server.incoming_requests() {
        handle(request, &state);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn local_request() -> tiny_http::TestRequest {
        tiny_http::TestRequest::new()
            .with_header(Header::from_bytes("Host", "127.0.0.1:4319").unwrap())
    }
    #[test]
    fn external_websites_and_rebound_hosts_cannot_access_local_logs() {
        let valid: Request = local_request()
            .with_header(Header::from_bytes("Origin", "http://127.0.0.1:1431").unwrap())
            .with_header(Header::from_bytes("Sec-Fetch-Site", "same-site").unwrap())
            .into();
        assert!(request_allowed(&valid));
        let website: Request = local_request()
            .with_header(Header::from_bytes("Origin", "https://example.com").unwrap())
            .into();
        assert!(!request_allowed(&website));
        let rebound: Request = tiny_http::TestRequest::new()
            .with_header(Header::from_bytes("Host", "attacker.example:4319").unwrap())
            .into();
        assert!(!request_allowed(&rebound));
        let cross_site: Request = local_request()
            .with_header(Header::from_bytes("Sec-Fetch-Site", "cross-site").unwrap())
            .into();
        assert!(!request_allowed(&cross_site));
    }
    #[test]
    fn form_posts_cannot_mutate_sources() {
        let mut form: Request = local_request()
            .with_body(r#"{"command":"source_save"}"#)
            .with_header(Header::from_bytes("Content-Type", "text/plain").unwrap())
            .into();
        assert!(read_json(&mut form).is_err());
        let mut json_request: Request = local_request()
            .with_body(r#"{"command":"snapshot"}"#)
            .with_header(
                Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap(),
            )
            .into();
        assert_eq!(read_json(&mut json_request).unwrap()["command"], "snapshot");
    }
    #[test]
    fn origin_allowlist_is_exact() {
        assert!(origin_allowed("http://127.0.0.1:1431"));
        assert!(origin_allowed("http://localhost:1431"));
        assert!(!origin_allowed("https://localhost:1431"));
        assert!(!origin_allowed("http://localhost:1431.evil.test"));
        assert!(!origin_allowed("null"));
    }
}
