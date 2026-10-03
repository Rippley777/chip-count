use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const PARSER: &str = "chip-count/1.0";
pub fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn timestamp(v: &Value) -> Option<String> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            t.with_timezone(&Utc)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
}
pub fn str_at(v: &Value, name: &str) -> Option<String> {
    v.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
pub fn num(v: &Value, name: &str) -> u64 {
    v.get(name).and_then(Value::as_u64).unwrap_or(0)
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
    pub total: u64,
    pub cost: f64,
    pub unpriced_tokens: u64,
    pub events: u64,
    #[serde(default)]
    pub unknown_fields: Vec<String>,
    #[serde(skip)]
    pub nano: i64,
}
impl Usage {
    pub fn tokens(input: u64, output: u64, read: u64, write: u64, reasoning: u64) -> Self {
        Self {
            input,
            output,
            cache_read: read,
            cache_write: write,
            reasoning,
            total: input
                .saturating_add(output)
                .saturating_add(read)
                .saturating_add(write),
            ..Self::default()
        }
    }
    pub fn add(&mut self, b: &Self) {
        for field in &b.unknown_fields {
            if !self.unknown_fields.contains(field) {
                self.unknown_fields.push(field.clone());
            }
        }
        self.input = self.input.saturating_add(b.input);
        self.output = self.output.saturating_add(b.output);
        self.cache_read = self.cache_read.saturating_add(b.cache_read);
        self.cache_write = self.cache_write.saturating_add(b.cache_write);
        self.reasoning = self.reasoning.saturating_add(b.reasoning);
        self.total = self.total.saturating_add(b.total);
        self.nano = self.nano.saturating_add(b.nano);
        self.cost = self.nano as f64 / 1e9;
        self.unpriced_tokens = self.unpriced_tokens.saturating_add(b.unpriced_tokens);
        self.events = self.events.saturating_add(b.events);
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub provider: String,
    pub raw_id: String,
    pub parent_id: Option<String>,
    pub project_path: String,
    pub first_at: String,
    pub last_at: String,
    pub source_path: String,
    pub completed: bool,
    pub warnings: Vec<String>,
    pub context_used: Option<u64>,
    pub context_capacity: Option<u64>,
    pub context_at: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub session_id: String,
    pub timestamp: String,
    pub model: String,
    pub usage: Usage,
    pub scope: String,
    pub kind: String,
    pub request_id: Option<String>,
    pub turn_id: Option<String>,
    pub source_path: String,
    pub source_line: u64,
    pub parser_version: String,
    pub pricing_version: Option<String>,
    pub reported: bool,
    pub warnings: Vec<String>,
    pub duration_ms: Option<u64>,
    #[serde(default = "now")]
    pub ingested_at: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParseState {
    pub session: SessionMeta,
    pub model: String,
    pub turn: Option<String>,
    pub counters: Option<Counters>,
    pub start_seen: bool,
    pub sequence: u64,
    pub inherited_before: Option<String>,
    pub generation: u64,
    #[serde(default)]
    pub inherited_ordinal: Option<u64>,
    #[serde(default)]
    pub service_tier: Option<String>,
    #[serde(default)]
    pub pending_responses: std::collections::BTreeMap<String, PendingResponse>,
    #[serde(default)]
    pub last_response: Option<String>,
    #[serde(default)]
    pub emitted_responses: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub compacted_responses: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub last_emitted_counter: Option<Counters>,
    #[serde(default)]
    pub request_baselines: std::collections::BTreeMap<String, Counters>,
    #[serde(default)]
    pub request_event_ids: std::collections::BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingResponse {
    pub event: Event,
    pub total: Option<Counters>,
    pub raw: Counters,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counters {
    pub input: u64,
    pub output: u64,
    pub cached: u64,
    pub reasoning: u64,
    #[serde(default)]
    pub written: u64,
    #[serde(default)]
    pub missing: Vec<String>,
}
impl Counters {
    pub fn parse(v: &Value) -> Self {
        let mut missing = vec![];
        for (raw, canonical) in [
            ("input_tokens", "input"),
            ("output_tokens", "output"),
            ("cached_input_tokens", "cache_read"),
            ("reasoning_output_tokens", "reasoning"),
        ] {
            if v.get(raw).and_then(Value::as_u64).is_none() {
                missing.push(canonical.into());
            }
        }
        // cache_write_input_tokens was added later and is explicitly defaulted by upstream.
        Self {
            input: num(v, "input_tokens"),
            output: num(v, "output_tokens"),
            cached: num(v, "cached_input_tokens"),
            reasoning: num(v, "reasoning_output_tokens"),
            written: v
                .get("cache_write_input_tokens")
                .or_else(|| v.get("cache_creation_input_tokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            missing,
        }
    }
    pub fn usage(&self) -> Usage {
        let read = self.cached.min(self.input);
        let write = self.written.min(self.input.saturating_sub(read));
        let mut u = Usage::tokens(
            self.input.saturating_sub(read).saturating_sub(write),
            self.output,
            read,
            write,
            self.reasoning.min(self.output),
        );
        u.unknown_fields = self.missing.clone();
        u
    }
    pub fn sub(&self, b: &Self) -> Self {
        Self {
            input: self.input.saturating_sub(b.input),
            output: self.output.saturating_sub(b.output),
            cached: self.cached.saturating_sub(b.cached),
            reasoning: self.reasoning.saturating_sub(b.reasoning),
            written: self.written.saturating_sub(b.written),
            missing: self.missing.clone(),
        }
    }
    pub fn lower(&self, b: &Self) -> bool {
        self.input < b.input || self.output < b.output
    }
    pub fn same_counts(&self, b: &Self) -> bool {
        self.input == b.input
            && self.output == b.output
            && self.cached == b.cached
            && self.written == b.written
            && self.reasoning == b.reasoning
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub model: String,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub version: String,
    pub source: String,
    pub retrieved_at: String,
    #[serde(rename = "override")]
    pub overridden: bool,
}
pub fn settings_default() -> Value {
    json!({"theme":"dark","density":"comfortable","timezone":"UTC","inactivity_minutes":10,"close_to_tray":true,"launch_at_login":false,"notifications":false,"retention_days":365,"redact_paths":false,"redact_labels":false,"monthly_subscription":null})
}
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Filter {
    pub search: Option<String>,
    pub provider: Option<String>,
    pub profile: Option<String>,
    pub project: Option<String>,
    pub model: Option<String>,
    pub state: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub tag: Option<String>,
    pub min_tokens: Option<u64>,
    pub max_tokens: Option<u64>,
    pub min_cost: Option<f64>,
    pub max_cost: Option<f64>,
    pub pinned: Option<bool>,
    pub sort: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}
