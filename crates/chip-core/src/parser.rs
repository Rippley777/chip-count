//! Metadata-only adapters. Source text, prompts, tool arguments and credentials are never retained.
use crate::model::*;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Default)]
pub struct Parsed {
    pub events: Vec<Event>,
    pub marker: Option<Value>,
    pub limits: Vec<Value>,
    pub recognized: bool,
    pub warning: Option<&'static str>,
}
fn namespaced(provider: &str, id: &str) -> String {
    format!("{provider}:{id}")
}
fn event(
    state: &ParseState,
    path: &str,
    line: u64,
    ts: &str,
    key: String,
    usage: Usage,
    kind: &str,
) -> Event {
    Event {
        id: hash(&format!("{}:{key}", state.session.id)),
        session_id: state.session.id.clone(),
        timestamp: ts.into(),
        model: if state.model.is_empty() {
            "unknown".into()
        } else {
            state.model.clone()
        },
        usage,
        scope: "session-own".into(),
        kind: kind.into(),
        request_id: None,
        turn_id: state.turn.clone(),
        source_path: path.into(),
        source_line: line,
        parser_version: PARSER.into(),
        pricing_version: None,
        reported: true,
        warnings: vec![],
        duration_ms: None,
        ingested_at: now(),
    }
}
fn touch(state: &mut ParseState, ts: &str) {
    if state.session.first_at.is_empty() || ts < state.session.first_at.as_str() {
        state.session.first_at = ts.into();
    }
    if state.session.last_at.is_empty() || ts > state.session.last_at.as_str() {
        state.session.last_at = ts.into();
    }
}
fn set_model(state: &mut ParseState, model: Option<String>, ts: Option<&str>, out: &mut Parsed) {
    if let Some(model) = model {
        if !state.model.is_empty() && state.model != model {
            if let Some(t) = ts {
                out.marker = Some(
                    json!({"timestamp":t,"kind":"model-switch","label":format!("{} → {}",state.model,model)}),
                );
            }
        }
        state.model = model;
    }
}
fn assumptions(e: &mut Event, state: &ParseState) {
    if !e.usage.unknown_fields.is_empty() {
        e.warnings.push(
            "Incomplete token categories: total is a lower bound; unknown categories are preserved"
                .into(),
        );
    }
    if state
        .service_tier
        .as_deref()
        .is_some_and(|s| !matches!(s, "default" | "standard" | "auto"))
    {
        e.warnings.push(format!(
            "Pricing unavailable: recorded service tier {} is not in the standard snapshot",
            state.service_tier.as_deref().unwrap_or("unknown")
        ));
    }
    if e.usage.input + e.usage.cache_read + e.usage.cache_write > 272_000
        && e.model.starts_with("gpt-")
    {
        e.warnings.push(
            "Pricing unavailable: long-context tier requires a verified rate override".into(),
        );
    }
}
fn inherited(state: &ParseState, v: &Value, line: u64, t: Option<&str>) -> bool {
    state
        .inherited_ordinal
        .is_some_and(|boundary| v["ordinal"].as_u64().unwrap_or(line.saturating_sub(1)) < boundary)
        || state
            .inherited_before
            .as_deref()
            .zip(t)
            .is_some_and(|(start, t)| t < start)
}
pub fn parse(provider: &str, v: &Value, state: &mut ParseState, path: &str, line: u64) -> Parsed {
    let mut out = Parsed::default();
    let ts = timestamp(&v["timestamp"]).or_else(|| timestamp(&v["ts"]));
    let typ = v["type"].as_str().unwrap_or("");
    if state.session.id.is_empty() {
        let stem = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("import");
        state.session = SessionMeta {
            id: namespaced(provider, stem),
            raw_id: stem.into(),
            provider: provider.into(),
            source_path: path.into(),
            ..SessionMeta::default()
        };
    }
    if provider == "codex" {
        parse_codex(v, typ, ts.as_deref(), state, path, line, &mut out);
    } else {
        parse_claude(v, typ, ts.as_deref(), state, path, line, &mut out);
    }
    if out.recognized && !inherited(state, v, line, ts.as_deref()) {
        if let Some(ts) = ts {
            touch(state, &ts);
        }
    }
    out
}
fn parse_codex(
    v: &Value,
    typ: &str,
    ts: Option<&str>,
    state: &mut ParseState,
    path: &str,
    line: u64,
    out: &mut Parsed,
) {
    let p = &v["payload"];
    if typ == "session_meta" {
        if let Some(id) = str_at(p, "id") {
            let canonical = namespaced("codex", &id);
            if canonical != state.session.id {
                *state = ParseState {
                    session: SessionMeta {
                        id: canonical,
                        raw_id: id,
                        provider: "codex".into(),
                        source_path: path.into(),
                        ..SessionMeta::default()
                    },
                    ..ParseState::default()
                };
            }
        }
        if let Some(cwd) = str_at(p, "cwd") {
            state.session.project_path = cwd;
        }
        let parent = str_at(p, "parent_thread_id")
            .or_else(|| str_at(p, "forked_from_id"))
            .or_else(|| {
                p.pointer("/source/subagent/thread_spawn/parent_thread_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| str_at(p, "parent_session_id"));
        state.session.parent_id = parent.map(|s| namespaced("codex", &s));
        state.inherited_ordinal = p["subagent_history_start_ordinal"].as_u64();
        if state.session.parent_id.is_some() && state.inherited_ordinal.is_none() {
            state.session.warnings.push("Legacy child history has no ordinal boundary: timestamp and response identity reconciliation may leave rewritten inherited usage incompletely attributed".into());
        }
        if state.session.parent_id.is_some() {
            state.inherited_before = timestamp(&p["timestamp"]).or_else(|| ts.map(str::to_string));
        }
        if p.get("history_base").is_some_and(|b| b.is_object()) {
            state.session.warnings.push("This rollout references external inherited history; only locally observed own usage is counted".into());
        }
        state.start_seen = true;
        out.recognized = true;
        return;
    }
    if typ == "thread.started" {
        if let Some(id) = str_at(v, "thread_id") {
            state.session.id = namespaced("codex", &id);
            state.session.raw_id = id;
        }
        state.start_seen = true;
        out.recognized = true;
        return;
    }
    if typ == "turn_context" {
        set_model(state, str_at(p, "model"), ts, out);
        if let Some(cwd) = str_at(p, "cwd") {
            state.session.project_path = cwd;
        }
        state.turn = str_at(p, "turn_id").or(state.turn.clone());
        if let Some(tier) = str_at(p, "service_tier") {
            state.service_tier = Some(tier);
        }
        out.recognized = true;
        return;
    }
    let inherited = inherited(state, v, line, ts);
    if typ == "token_usage_record" {
        out.recognized = true;
        if inherited {
            return;
        }
        let (Some(t), Some(id)) = (ts, str_at(p, "response_id")) else {
            out.warning = Some("Response usage without timestamp or response ID was not counted");
            return;
        };
        if state.emitted_responses.contains(&id) {
            return;
        }
        let raw = Counters::parse(&p["usage"]);
        let total = p
            .get("thread_token_usage")
            .filter(|x| x.is_object())
            .map(Counters::parse);
        // Some versions emit the response record after the cumulative snapshot.
        // It establishes identity but cannot add the same usage a second time.
        if total
            .as_ref()
            .zip(state.last_emitted_counter.as_ref())
            .is_some_and(|(a, b)| a.same_counts(b))
        {
            state.emitted_responses.insert(id);
            return;
        }
        let mut e = event(
            state,
            path,
            line,
            t,
            format!("response:{id}"),
            raw.usage(),
            "response-record",
        );
        e.id = hash(&format!("codex:response:{id}"));
        e.request_id = Some(id.clone());
        e.turn_id = str_at(p, "turn_id").or(state.turn.clone());
        if let Some(model) = str_at(p, "model") {
            e.model = model;
        }
        assumptions(&mut e, state);
        let pending = PendingResponse {
            event: e,
            total,
            raw,
        };
        state.last_response = Some(id.clone());
        if state.compacted_responses.contains(&id) {
            out.events.push(pending.event);
            state.emitted_responses.insert(id);
        } else {
            state.pending_responses.insert(id, pending);
        }
        // Outstanding request metadata is bounded. Ordinary records are accounted by token_count.
        while state.pending_responses.len() > 64 {
            if let Some(k) = state.pending_responses.keys().next().cloned() {
                state.pending_responses.remove(&k);
            }
        }
        return;
    }
    if typ == "compacted" {
        out.recognized = true;
        if inherited {
            return;
        }
        out.marker =
            ts.map(|t| json!({"timestamp":t,"kind":"compaction","label":"Context compacted"}));
        if let Some(id) = str_at(p, "compaction_response_id") {
            state.compacted_responses.insert(id.clone());
            if !state.emitted_responses.contains(&id) {
                if let Some(mut pending) = state.pending_responses.remove(&id) {
                    pending.event.kind = "compaction-response".into();
                    out.events.push(pending.event);
                    state.emitted_responses.insert(id);
                }
            }
        }
        return;
    }
    if typ == "turn.completed" && v["usage"].is_object() {
        out.recognized = true;
        let Some(t) = ts else {
            out.warning = Some("Headless usage has no timestamp; not assigned an invented date");
            return;
        };
        set_model(state, str_at(v, "model"), ts, out);
        state.sequence += 1;
        let request = str_at(v, "turn_id").or_else(|| str_at(v, "id"));
        let key = request
            .clone()
            .unwrap_or_else(|| format!("{t}:{}", state.sequence));
        let mut e = event(
            state,
            path,
            line,
            t,
            format!("exec:{key}"),
            Counters::parse(&v["usage"]).usage(),
            "request-delta",
        );
        e.request_id = request;
        assumptions(&mut e, state);
        out.events.push(e);
        return;
    }
    if typ != "event_msg" {
        return;
    }
    match p["type"].as_str().unwrap_or("") {
        "task_started" | "turn_started" => {
            state.turn = str_at(p, "turn_id").or(state.turn.clone());
            state.session.completed = false;
            out.recognized = true;
        }
        "task_complete" | "turn_complete" => {
            out.recognized = true;
        } // A completed turn is not a completed session.
        "session_complete" => {
            state.session.completed = true;
            out.recognized = true;
        }
        "thread_settings_applied" => {
            if let Some(tier) = p
                .pointer("/thread_settings/service_tier")
                .and_then(Value::as_str)
            {
                state.service_tier = Some(tier.into());
            }
            out.recognized = true;
        }
        "context_compacted" => {
            out.recognized = true;
            if !inherited {
                out.marker = ts.map(
                    |t| json!({"timestamp":t,"kind":"compaction","label":"Context compacted"}),
                );
            }
        }
        "error" | "retry" => {
            out.recognized = true;
            if !inherited {
                out.marker=ts.map(|t|json!({"timestamp":t,"kind":p["type"],"label":"Provider reported an error or retry"}));
            }
        }
        "token_count" => {
            out.recognized = true;
            if let (Some(limits), Some(t)) = (p.get("rate_limits").filter(|x| x.is_object()), ts) {
                if !inherited {
                    for name in ["primary", "secondary"] {
                        let lim = &limits[name];
                        if let Some(used) = lim["used_percent"]
                            .as_f64()
                            .filter(|x| x.is_finite() && *x >= 0.)
                        {
                            out.limits.push(json!({"provider":"codex","scope":format!("{} / {} / observed in {}",limits["limit_id"].as_str().unwrap_or("reported"),name,state.session.raw_id),"used_percent":used,"resets_at":lim["resets_at"].as_i64().and_then(|x|chrono::DateTime::from_timestamp(x,0)).map(|x|x.to_rfc3339()),"observed_at":t,"window_minutes":lim["window_minutes"]}));
                        }
                    }
                }
            }
            let info = &p["info"];
            if info.is_null() {
                return;
            }
            let Some(t) = ts else {
                out.warning = Some("Usage record lacks a valid timestamp; omitted from totals");
                return;
            };
            if let Some(cap) = info["model_context_window"].as_u64() {
                state.session.context_capacity = Some(cap);
            }
            // Only explicit context occupancy is used. last_token_usage remains throughput.
            if let Some(used) = info["context_usage_tokens"].as_u64() {
                state.session.context_used = Some(used);
                state.session.context_at = Some(t.into());
            }
            set_model(
                state,
                str_at(p, "model").or_else(|| str_at(info, "model")),
                ts,
                out,
            );
            let mut total = info
                .get("total_token_usage")
                .filter(|x| x.is_object())
                .map(Counters::parse);
            // An omitted cumulative category is unknown, not a reset to zero.
            // Keep its last established baseline so the next complete report does
            // not bill that already-observed category a second time.
            if let (Some(total), Some(previous)) = (&mut total, &state.counters) {
                for field in &total.missing {
                    match field.as_str() {
                        "input" => total.input = previous.input,
                        "output" => total.output = previous.output,
                        "cache_read" => total.cached = previous.cached,
                        "cache_write" => total.written = previous.written,
                        "reasoning" => total.reasoning = previous.reasoning,
                        _ => {}
                    }
                }
            }
            let last = info
                .get("last_token_usage")
                .filter(|x| x.is_object())
                .map(Counters::parse);
            if inherited {
                if total.is_some() {
                    state.counters = total;
                }
                return;
            }
            if total.is_none() && last.is_none() {
                return;
            }
            state.sequence += 1;
            let request = str_at(p, "request_id").or_else(|| str_at(p, "response_id"));
            let existing_baseline = request
                .as_ref()
                .and_then(|r| state.request_baselines.get(r))
                .cloned();
            if let Some(id) = &request {
                if !state.request_baselines.contains_key(id) {
                    let base = state.counters.clone().unwrap_or_else(|| {
                        total
                            .as_ref()
                            .zip(last.as_ref())
                            .map(|(t, l)| t.sub(l))
                            .unwrap_or_default()
                    });
                    state.request_baselines.insert(id.clone(), base);
                    if state.request_baselines.len() > 512 {
                        if let Some(k) = state.request_baselines.keys().next().cloned() {
                            state.request_baselines.remove(&k);
                        }
                    }
                }
            }
            let mut warnings = vec![];
            let mut carry = None;
            let delta = if let (Some(total), Some(base)) = (&total, &existing_baseline) {
                warnings
                    .push("Corrected response snapshot replaces the earlier contribution".into());
                total.sub(base)
            } else {
                match (&total, &state.counters) {
                    (Some(total), Some(prev)) if total.same_counts(prev) => return,
                    (Some(total), Some(prev)) if total.lower(prev) => {
                        state.generation += 1;
                        warnings.push("Cumulative counter decreased: reported latest usage starts a new segment; an unknown balance is not invented".into());
                        if let Some(last) = &last {
                            carry = Some(total.sub(last));
                            last.clone()
                        } else {
                            carry = Some(total.clone());
                            Counters::default()
                        }
                    }
                    (Some(total), Some(prev)) => total.sub(prev),
                    (Some(total), None) => {
                        if let Some(last) = &last {
                            if !total.same_counts(last) {
                                carry = Some(total.sub(last));
                                warnings.push("Opening cumulative balance includes historical carry-in, kept separate from observed usage".into());
                            }
                            last.clone()
                        } else if state.start_seen && state.session.parent_id.is_none() {
                            total.clone()
                        } else {
                            carry = Some(total.clone());
                            warnings.push("Partial history: opening cumulative balance retained as historical carry-in".into());
                            Counters::default()
                        }
                    }
                    (None, _) => {
                        warnings.push("Delta-only usage; weak identity uses session, timestamp and turn when no request ID exists".into());
                        last.clone().unwrap_or_default()
                    }
                }
            };
            if total.is_some() {
                state.counters = total.clone();
            }
            let key = if let Some(id) = &request {
                format!("request:{id}")
            } else if let Some(total) = &total {
                format!(
                    "counter:{t}:{}:{}:{}:{}:{}",
                    total.input, total.output, total.cached, total.written, total.reasoning
                )
            } else {
                format!(
                    "delta:{t}:{}:{}",
                    state.turn.as_deref().unwrap_or(""),
                    state.sequence
                )
            };
            if let Some(carry) = carry.filter(|c| c.usage().total > 0) {
                let mut e = event(
                    state,
                    path,
                    line,
                    t,
                    format!("carry:{key}"),
                    carry.usage(),
                    "historical-carry-in",
                );
                e.scope = "carry-in".into();
                e.reported = false;
                out.events.push(e);
            }
            // A known response corrected to zero must replace its former event.
            // Missing categories retain their unknown markers for insertion-time
            // reconciliation with previously reported fields.
            if delta.usage().total == 0 && existing_baseline.is_none() {
                return;
            }
            let mut e = event(
                state,
                path,
                line,
                t,
                key,
                delta.usage(),
                if total.is_some() {
                    "cumulative-difference"
                } else {
                    "request-delta"
                },
            );
            e.reported = total.is_none();
            e.request_id = request.clone();
            e.warnings = warnings;
            if let Some(id) = state.last_response.clone() {
                if let Some(pending) = state.pending_responses.get(&id) {
                    if pending.raw.same_counts(&delta)
                        || pending
                            .total
                            .as_ref()
                            .zip(total.as_ref())
                            .is_some_and(|(a, b)| a.same_counts(b))
                    {
                        e.id = pending.event.id.clone();
                        e.request_id = Some(id.clone());
                        e.turn_id = pending.event.turn_id.clone();
                        state.pending_responses.remove(&id);
                        state.emitted_responses.insert(id);
                    }
                }
            }
            // A response-record match may have promoted the first observation to
            // a global response identity. Corrections must replace that same row,
            // even after the pending response has been consumed or after restart.
            if let Some(request) = request {
                if let Some(canonical) = state.request_event_ids.get(&request) {
                    e.id = canonical.clone();
                } else {
                    state.request_event_ids.insert(request, e.id.clone());
                }
                while state.request_event_ids.len() > 512 {
                    if let Some(key) = state.request_event_ids.keys().next().cloned() {
                        state.request_event_ids.remove(&key);
                    }
                }
            }
            if delta.cached.saturating_add(delta.written) > delta.input
                || delta.reasoning > delta.output
            {
                e.warnings
                    .push("Invalid subset counters capped at the enclosing category".into());
            }
            assumptions(&mut e, state);
            out.events.push(e);
            state.last_emitted_counter = total;
            // Bound checkpoint metadata independently of session lifetime.
            if state.emitted_responses.len() > 2048 {
                state.emitted_responses.clear();
            }
        }
        _ => {}
    }
}
fn parse_claude(
    v: &Value,
    typ: &str,
    ts: Option<&str>,
    state: &mut ParseState,
    path: &str,
    line: u64,
    out: &mut Parsed,
) {
    if let Some(id) = str_at(v, "sessionId").or_else(|| str_at(v, "session_id")) {
        let agent = str_at(v, "agentId").or_else(|| {
            Path::new(path)
                .file_stem()
                .and_then(|x| x.to_str())
                .filter(|s| s.starts_with("agent-"))
                .map(|s| s.trim_start_matches("agent-").to_string())
        });
        let (canonical, raw, parent) = if let Some(agent) = agent {
            (
                namespaced("claude", &format!("{id}/agent:{agent}")),
                agent,
                Some(namespaced("claude", &id)),
            )
        } else {
            (namespaced("claude", &id), id, None)
        };
        if canonical != state.session.id {
            state.session = SessionMeta {
                id: canonical,
                raw_id: raw,
                parent_id: parent,
                provider: "claude".into(),
                source_path: path.into(),
                ..SessionMeta::default()
            };
            state.model.clear();
            state.turn = None;
        }
    }
    if let Some(parent) = str_at(v, "parentSessionId") {
        state.session.parent_id = Some(namespaced("claude", &parent));
    }
    if let Some(cwd) = str_at(v, "cwd") {
        state.session.project_path = cwd;
    }
    if typ == "system" && v["subtype"].as_str() == Some("compact_boundary") {
        out.recognized = true;
        out.marker =
            ts.map(|t| json!({"timestamp":t,"kind":"compaction","label":"Context compacted"}));
    }
    if typ == "session-end" {
        state.session.completed = true;
        out.recognized = true;
    }
    let msg = &v["message"];
    if (typ != "assistant" && msg["role"].as_str() != Some("assistant"))
        || !msg["usage"].is_object()
    {
        return;
    }
    out.recognized = true;
    let Some(t) = ts else {
        out.warning = Some("Usage record lacks a valid timestamp; omitted from totals");
        return;
    };
    let usage = &msg["usage"];
    set_model(
        state,
        str_at(msg, "model").or(Some("unknown".into())),
        ts,
        out,
    );
    if state.model == "<synthetic>" {
        return;
    }
    let mid = str_at(msg, "id");
    let request = str_at(v, "requestId").or_else(|| str_at(v, "request_id"));
    let fallback = str_at(v, "uuid");
    let key = match (&mid, &request, &fallback) {
        (Some(m), Some(r), _) => format!("response:{m}:{r}"),
        (Some(m), _, _) => format!("response:{m}"),
        (_, Some(r), _) => format!("request:{r}"),
        (_, _, Some(u)) => format!("uuid:{u}"),
        _ => format!("fallback:{t}:{line}"),
    };
    let creation = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            num(&usage["cache_creation"], "ephemeral_5m_input_tokens")
                + num(&usage["cache_creation"], "ephemeral_1h_input_tokens")
        });
    let tokens = Usage::tokens(
        num(usage, "input_tokens"),
        num(usage, "output_tokens"),
        num(usage, "cache_read_input_tokens"),
        creation,
        0,
    );
    let final_record = msg["stop_reason"].is_string() || v["isFinal"].as_bool() == Some(true);
    let mut e = event(
        state,
        path,
        line,
        t,
        key.clone(),
        tokens,
        if final_record {
            "response-final"
        } else {
            "response-snapshot"
        },
    );
    // Provider message/request identities survive replay into another session.
    if mid.is_some() || request.is_some() || fallback.is_some() {
        e.id = hash(&format!("claude:{key}"));
    }
    e.request_id = request;
    e.turn_id = mid;
    e.duration_ms = v["durationMs"]
        .as_u64()
        .or_else(|| v["duration_ms"].as_u64());
    for (raw, canonical) in [
        ("input_tokens", "input"),
        ("output_tokens", "output"),
        ("cache_read_input_tokens", "cache_read"),
    ] {
        if usage.get(raw).and_then(Value::as_u64).is_none() {
            e.usage.unknown_fields.push(canonical.into());
        }
    }
    if usage.get("cache_creation_input_tokens").is_none() && !usage["cache_creation"].is_object() {
        e.usage.unknown_fields.push("cache_write".into());
    }
    if let Some(speed) = str_at(usage, "speed") {
        if speed == "fast" {
            e.warnings.push("Pricing unavailable: Claude fast mode is outside the bundled standard rate snapshot".into());
        }
    }
    if num(&usage["cache_creation"], "ephemeral_1h_input_tokens") > 0 {
        e.warnings.push(
            "Pricing unavailable: one-hour cache write rate requires a verified override".into(),
        );
    }
    if fallback.is_none() && e.request_id.is_none() && e.turn_id.is_none() {
        e.warnings.push("No stable event ID: identity falls back to session, timestamp and source line; shifted excerpts may not deduplicate".into());
    }
    if v["usageScope"].as_str() == Some("combined") || usage["scope"].as_str() == Some("combined") {
        e.scope = "parent-rollup".into();
        e.warnings
            .push("Parent rollup retained for provenance; excluded from own/global totals".into());
    }
    assumptions(&mut e, state);
    out.events.push(e);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn usage(input: u64, output: u64) -> Value {
        json!({"input_tokens":input,"output_tokens":output,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":input+output})
    }
    fn meta() -> Value {
        json!({"type":"session_meta","timestamp":"2026-10-01T10:00:00Z","payload":{"id":"session-a","cwd":"/test/a"}})
    }
    fn count(input: u64, output: u64, second: u8) -> Value {
        json!({"type":"event_msg","timestamp":format!("2026-10-01T10:00:{second:02}Z"),"payload":{"type":"token_count","info":{"total_token_usage":usage(input,output),"model_context_window":200000}}})
    }
    fn run(lines: Vec<Value>) -> (ParseState, Vec<Event>) {
        let mut state = ParseState::default();
        let mut events = vec![];
        for (i, line) in lines.into_iter().enumerate() {
            events
                .extend(parse("codex", &line, &mut state, "/test/log.jsonl", i as u64 + 1).events);
        }
        (state, events)
    }
    fn own(events: &[Event]) -> Usage {
        let mut result = Usage::default();
        for e in events.iter().filter(|e| e.scope == "session-own") {
            result.add(&e.usage);
        }
        result
    }
    #[test]
    fn cumulative_snapshots_are_increments_not_addends() {
        let (_, events) = run(vec![meta(), count(100, 10, 1), count(150, 25, 2)]);
        let u = own(&events);
        assert_eq!((u.input, u.output, u.total), (150, 25, 175));
    }
    #[test]
    fn latest_delta_and_cumulative_total_count_once() {
        let mut a = count(100, 10, 1);
        a["payload"]["info"]["last_token_usage"] = usage(100, 10);
        let mut b = count(150, 25, 2);
        b["payload"]["info"]["last_token_usage"] = usage(50, 15);
        let (_, events) = run(vec![meta(), a, b.clone(), b]);
        assert_eq!(own(&events).total, 175);
    }
    #[test]
    fn response_metadata_reconciles_with_the_cumulative_record() {
        let record = json!({"type":"token_usage_record","timestamp":"2026-10-01T10:00:01Z","payload":{"response_id":"r1","usage":usage(100,10),"thread_token_usage":usage(100,10)}});
        let (_, events) = run(vec![meta(), record, count(100, 10, 1)]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].request_id.as_deref(), Some("r1"));
        assert_eq!(own(&events).total, 110);
    }
    #[test]
    fn remote_compaction_pair_counts_but_unmatched_metadata_does_not() {
        let record = json!({"type":"token_usage_record","timestamp":"2026-10-01T10:00:02Z","payload":{"response_id":"compaction-r1","usage":usage(40,4)}});
        let marker = json!({"type":"compacted","timestamp":"2026-10-01T10:00:03Z","payload":{"compaction_response_id":"compaction-r1"}});
        let (_, events) = run(vec![
            meta(),
            count(100, 10, 1),
            record.clone(),
            marker.clone(),
            marker,
        ]);
        assert_eq!(own(&events).total, 154);
        let (_, events) = run(vec![meta(), record]);
        assert_eq!(own(&events).total, 0);
    }
    #[test]
    fn local_compaction_pair_does_not_duplicate_counter_usage() {
        let record = json!({"type":"token_usage_record","timestamp":"2026-10-01T10:00:01Z","payload":{"response_id":"r1","usage":usage(100,10),"thread_token_usage":usage(100,10)}});
        let marker = json!({"type":"compacted","timestamp":"2026-10-01T10:00:02Z","payload":{"compaction_response_id":"r1"}});
        let (_, events) = run(vec![meta(), record, count(100, 10, 1), marker]);
        assert_eq!(own(&events).total, 110);
    }
    #[test]
    fn partial_history_balance_is_separate_even_with_a_copied_header() {
        let mut c = count(1000, 100, 1);
        c["payload"]["info"]["last_token_usage"] = usage(100, 10);
        let (_, events) = run(vec![meta(), c]);
        assert_eq!(own(&events).total, 110);
        assert_eq!(
            events
                .iter()
                .find(|e| e.scope == "carry-in")
                .unwrap()
                .usage
                .total,
            990
        );
    }
    #[test]
    fn current_subagent_ordinal_excludes_inherited_parent_tokens() {
        let mut m = meta();
        m["payload"]["id"] = json!("child");
        m["payload"]["parent_thread_id"] = json!("parent");
        m["payload"]["subagent_history_start_ordinal"] = json!(3);
        let (state, events) = run(vec![
            m,
            count(100, 10, 1),
            count(150, 25, 2),
            count(190, 30, 3),
        ]);
        assert_eq!(state.session.parent_id.as_deref(), Some("codex:parent"));
        assert_eq!(own(&events).total, 45);
    }
    #[test]
    fn cache_and_reasoning_subsets_never_inflate_throughput() {
        let mut c = count(100, 30, 1);
        c["payload"]["info"]["total_token_usage"]["cached_input_tokens"] = json!(50);
        c["payload"]["info"]["total_token_usage"]["cache_write_input_tokens"] = json!(10);
        c["payload"]["info"]["total_token_usage"]["reasoning_output_tokens"] = json!(20);
        let (state, events) = run(vec![meta(), c]);
        let u = own(&events);
        assert_eq!(
            (
                u.input,
                u.cache_read,
                u.cache_write,
                u.output,
                u.reasoning,
                u.total
            ),
            (40, 50, 10, 30, 20, 130)
        );
        assert_eq!(state.session.context_used, None);
        assert_eq!(state.session.context_capacity, Some(200000));
    }
    #[test]
    fn distinct_claude_requests_with_identical_usage_remain_distinct() {
        let mut state = ParseState::default();
        let mut v = json!({"timestamp":"2026-10-01T10:00:01Z","sessionId":"a","requestId":"r1","message":{"role":"assistant","id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":10}}});
        let a = parse("claude", &v, &mut state, "/test/a.jsonl", 1)
            .events
            .remove(0);
        v["requestId"] = json!("r2");
        let b = parse("claude", &v, &mut state, "/test/a.jsonl", 2)
            .events
            .remove(0);
        assert_ne!(a.id, b.id);
        assert_eq!(a.usage.total + b.usage.total, 220);
        assert!(a.usage.unknown_fields.contains(&"cache_read".into()));
    }
    #[test]
    fn claude_final_snapshot_preserves_response_identity_and_authority() {
        let mut state = ParseState::default();
        let mut v = json!({"type":"assistant","timestamp":"2026-10-01T10:00:01Z","sessionId":"a","requestId":"r1","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":1}}});
        let a = parse("claude", &v, &mut state, "/test/a.jsonl", 1)
            .events
            .remove(0);
        v["message"]["usage"]["output_tokens"] = json!(10);
        v["message"]["stop_reason"] = json!("end_turn");
        let b = parse("claude", &v, &mut state, "/test/a.jsonl", 2)
            .events
            .remove(0);
        assert_eq!(a.id, b.id);
        assert_eq!(b.kind, "response-final");
        assert_eq!(b.usage.output, 10);
    }
    #[test]
    fn corrected_request_snapshot_keeps_original_response_baseline() {
        let mut first = count(100, 10, 1);
        first["payload"]["request_id"] = json!("r1");
        let mut corrected = count(150, 25, 2);
        corrected["payload"]["request_id"] = json!("r1");
        let (_, events) = run(vec![meta(), first, corrected]);
        assert_eq!(events[0].id, events[1].id);
        assert_eq!(events[1].usage.total, 175);
    }
    #[test]
    fn reset_without_latest_usage_does_not_invent_increment() {
        let (_, events) = run(vec![meta(), count(100, 10, 1), count(30, 4, 2)]);
        assert_eq!(own(&events).total, 110);
        assert_eq!(events.iter().filter(|e| e.scope == "carry-in").count(), 1);
    }
    #[test]
    fn rate_limit_scope_is_bound_to_its_observation_not_directory_identity() {
        let mut c = count(100, 10, 1);
        c["payload"]["rate_limits"] = json!({"limit_id":"codex","primary":{"used_percent":48.,"window_minutes":300,"resets_at":1790860000}});
        let mut state = ParseState::default();
        parse("codex", &meta(), &mut state, "/a.jsonl", 1);
        let p = parse("codex", &c, &mut state, "/a.jsonl", 2);
        assert_eq!(p.limits[0]["used_percent"], 48.);
        assert!(p.limits[0]["scope"].as_str().unwrap().contains("session-a"));
    }
}
