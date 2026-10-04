use crate::calendar::{bound, resolve, Range};
use crate::{model::*, required, Engine};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Duration, Utc};
use chrono_tz::Tz;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
struct View {
    sessions: Vec<Value>,
    events: Vec<Event>,
    all: Vec<Event>,
    meta: HashMap<String, SessionMeta>,
    tz: Tz,
    range: Range,
}
fn total<'a>(events: impl Iterator<Item = &'a Event>) -> Usage {
    let mut u = Usage::default();
    for e in events {
        if e.scope == "session-own" {
            u.add(&e.usage);
        }
    }
    u
}
fn date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|x| x.with_timezone(&Utc))
}
fn in_range(e: &Event, from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> bool {
    date(&e.timestamp).is_some_and(|t| from.is_none_or(|x| t >= x) && to.is_none_or(|x| t < x))
}
fn buckets<'a>(
    events: impl Iterator<Item = &'a Event>,
    key: impl Fn(&Event) -> String,
) -> Vec<Value> {
    let mut groups: BTreeMap<String, Usage> = BTreeMap::new();
    for e in events {
        if e.scope == "session-own" {
            groups.entry(key(e)).or_default().add(&e.usage);
        }
    }
    groups
        .into_iter()
        .map(|(key, u)| {
            let mut v = serde_json::to_value(u).unwrap();
            v["key"] = json!(key);
            v["label"] = json!(key);
            v
        })
        .collect()
}
fn timeline(events: &[Event], tz: Tz, elapsed: Option<DateTime<Utc>>) -> Vec<Value> {
    let mut groups: BTreeMap<i64, Usage> = BTreeMap::new();
    for e in events.iter().filter(|e| e.scope == "session-own") {
        if let Some(t) = date(&e.timestamp) {
            let minute = elapsed
                .map(|start| (t - start).num_seconds().max(0) / 60)
                .unwrap_or(t.timestamp().div_euclid(60));
            groups.entry(minute).or_default().add(&e.usage);
        }
    }
    let keys: Vec<_> = groups.keys().copied().collect();
    for pair in keys.windows(2) {
        if pair[1] - pair[0] > 1 {
            groups.entry(pair[0] + 1).or_default();
            groups.entry(pair[1] - 1).or_default();
        }
    }
    groups
        .into_iter()
        .map(|(minute, u)| {
            let key = if elapsed.is_some() {
                format!("{minute:05}m")
            } else {
                DateTime::from_timestamp(minute * 60, 0)
                    .unwrap()
                    .with_timezone(&tz)
                    .format("%Y-%m-%d %H:%M %:z")
                    .to_string()
            };
            let mut v = serde_json::to_value(u).unwrap();
            v["key"] = json!(key);
            v["label"] = json!(key);
            v["x"] = json!(minute * 60_000);
            v
        })
        .collect()
}
fn daily_buckets(events: &[Event], tz: Tz, range: &Range) -> Vec<Value> {
    let existing = buckets(events.iter(), |e| {
        date(&e.timestamp)
            .unwrap()
            .with_timezone(&tz)
            .format("%Y-%m-%d")
            .to_string()
    });
    let mut groups: BTreeMap<String, Value> = existing
        .into_iter()
        .map(|v| (v["key"].as_str().unwrap().to_owned(), v))
        .collect();
    let first = range
        .from
        .or_else(|| events.iter().filter_map(|e| date(&e.timestamp)).min());
    let last = range
        .to
        .map(|t| {
            if range.from == Some(t) {
                t
            } else {
                t - Duration::nanoseconds(1)
            }
        })
        .or_else(|| events.iter().filter_map(|e| date(&e.timestamp)).max());
    if let Some((first, last)) = first.zip(last) {
        let mut day = first.with_timezone(&tz).date_naive();
        let last = last.with_timezone(&tz).date_naive();
        while day <= last {
            let key = day.to_string();
            let v = groups
                .entry(key.clone())
                .or_insert_with(|| serde_json::to_value(Usage::default()).unwrap());
            v["key"] = json!(key);
            v["label"] = json!(key);
            // Civil days have uniform daily spacing; minute timelines use elapsed instants.
            v["x"] = json!(day
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis());
            day = day.succ_opt().unwrap();
        }
    }
    groups.into_values().collect()
}
fn activity(events: &[&Event]) -> i64 {
    let mut times: Vec<_> = events.iter().filter_map(|e| date(&e.timestamp)).collect();
    times.sort();
    times.dedup();
    times
        .windows(2)
        .map(|w| (w[1] - w[0]).num_seconds().clamp(0, 300))
        .sum()
}
fn display_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Unassigned")
        .into()
}
fn clean_filter(f: &Filter) -> Value {
    serde_json::to_value(f).unwrap_or(json!({}))
}
impl Engine {
    fn view(&self, filter: &Filter) -> Result<View> {
        let settings = self.settings()?;
        let tz = settings["timezone"]
            .as_str()
            .unwrap_or("UTC")
            .parse::<Tz>()?;
        let range = resolve(filter, tz, Utc::now())?;
        let from = range.from;
        let to = range.to;
        let as_of = range.as_of;
        let all = self.events()?;
        let mut meta = HashMap::new();
        for v in self.list("sessions")? {
            let m: SessionMeta = serde_json::from_value(v)?;
            meta.insert(m.id.clone(), m);
        }
        let sources = self.list("sources")?;
        let source_labels: HashMap<_, _> = sources
            .iter()
            .filter(|s| s["enabled"] == true)
            .filter_map(|s| {
                Some((
                    s["id"].as_str()?.to_owned(),
                    s["label"].as_str()?.to_owned(),
                ))
            })
            .collect();
        let mut session_profiles: HashMap<String, HashSet<String>> = HashMap::new();
        let mut profile_events = HashSet::new();
        let mut provenance = self.db.prepare("SELECT DISTINCT e.session_id,o.source_id,e.id FROM events e JOIN origins o ON o.event_id=e.id")?;
        for row in provenance.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (session, source, event) = row?;
            if let Some(label) = source_labels.get(&source) {
                session_profiles
                    .entry(session)
                    .or_default()
                    .insert(label.clone());
                if filter
                    .profile
                    .as_deref()
                    .is_none_or(|p| p.is_empty() || p == label)
                {
                    profile_events.insert(event);
                }
            }
        }
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for m in meta.values() {
            if let Some(parent) = &m.parent_id {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(m.id.clone());
            }
        }
        let annotations: HashMap<_, _> = self
            .db
            .prepare("SELECT id,data FROM annotations")?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(k, v)| Ok((k, serde_json::from_str::<Value>(&v)?)))
            .collect::<Result<_>>()?;
        let projects = self.list("projects")?;
        let filtered: Vec<Event> = all
            .iter()
            .filter(|e| {
                in_range(e, from, to)
                    && profile_events.contains(&e.id)
                    && filter
                        .model
                        .as_ref()
                        .is_none_or(|m| m.is_empty() || m == &e.model)
            })
            .cloned()
            .collect();
        let mut own: HashMap<String, Vec<&Event>> = HashMap::new();
        for e in &filtered {
            if e.scope == "session-own" {
                own.entry(e.session_id.clone()).or_default().push(e);
            }
        }
        let mut present: HashSet<_> = all.iter().map(|e| e.session_id.clone()).collect();
        // Newly started sessions are useful before their first completed token report.
        // Durable file state establishes their source membership without indexing transcript content.
        let mut files = self
            .db
            .prepare("SELECT json_extract(state,'$.session.id'),source_id FROM files")?;
        for row in files.query_map([], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })? {
            let (id, source) = row?;
            if let (Some(id), Some(label)) = (id, source_labels.get(&source)) {
                let cutoff =
                    as_of - Duration::days(settings["retention_days"].as_i64().unwrap_or(365));
                if meta
                    .get(&id)
                    .is_some_and(|m| date(&m.last_at).is_some_and(|t| t >= cutoff))
                {
                    present.insert(id.clone());
                }
                session_profiles
                    .entry(id)
                    .or_default()
                    .insert(label.clone());
            }
        }
        let mut sessions = vec![];
        for m in meta.values() {
            if !present.contains(&m.id) {
                continue;
            }
            let events = own.get(&m.id).cloned().unwrap_or_default();
            let usage = total(events.iter().copied());
            if events.is_empty() {
                if filter.model.as_ref().is_some_and(|m| !m.is_empty()) {
                    continue;
                }
                if (from.is_some() || to.is_some())
                    && !date(&m.last_at)
                        .is_some_and(|t| from.is_none_or(|d| t >= d) && to.is_none_or(|d| t < d))
                {
                    continue;
                }
            }
            let source = sources
                .iter()
                .filter(|s| s["enabled"] == true)
                .filter(|s| {
                    s["path"]
                        .as_str()
                        .is_some_and(|p| std::path::Path::new(&m.source_path).starts_with(p))
                })
                .max_by_key(|s| s["path"].as_str().map(str::len).unwrap_or(0));
            let default_profile = source
                .and_then(|s| s["label"].as_str())
                .unwrap_or("Imported");
            let profiles = session_profiles.get(&m.id);
            let mut labels: Vec<_> = profiles.into_iter().flatten().cloned().collect();
            labels.sort();
            let joined_profile = labels.join(" · ");
            let profile = filter
                .profile
                .as_deref()
                .filter(|p| profiles.is_some_and(|ps| ps.contains(*p)))
                .unwrap_or(if joined_profile.is_empty() {
                    default_profile
                } else {
                    &joined_profile
                });
            let annotation = annotations.get(&m.id).cloned().unwrap_or(json!({}));
            let project = projects.iter().find(|p| {
                p["path"] == m.project_path
                    || p["aliases"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|x| x == &m.project_path))
            });
            let canonical = project
                .and_then(|p| p["path"].as_str())
                .unwrap_or(&m.project_path);
            let project_name = project
                .and_then(|p| p["name"].as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| display_path(&m.project_path));
            let name = annotation["alias"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    format!(
                        "{} · {}",
                        project_name,
                        m.raw_id.chars().take(8).collect::<String>()
                    )
                });
            let state = if m.completed {
                "completed"
            } else if date(&m.last_at).is_some_and(|d| {
                d <= as_of
                    && as_of - d
                        < Duration::minutes(settings["inactivity_minutes"].as_i64().unwrap_or(10))
            }) {
                "active"
            } else {
                "idle"
            };
            let tags = annotation.get("tags").cloned().unwrap_or(json!([]));
            let pinned = annotation["pinned"].as_bool().unwrap_or(false);
            if filter
                .provider
                .as_ref()
                .is_some_and(|s| !s.is_empty() && s != &m.provider)
                || filter
                    .profile
                    .as_ref()
                    .is_some_and(|s| !s.is_empty() && !profiles.is_some_and(|ps| ps.contains(s)))
                || filter
                    .project
                    .as_ref()
                    .is_some_and(|s| !s.is_empty() && s != canonical && s != &m.project_path)
                || filter
                    .state
                    .as_ref()
                    .is_some_and(|s| !s.is_empty() && s != state)
                || filter
                    .tag
                    .as_ref()
                    .is_some_and(|s| !tags.as_array().is_some_and(|a| a.contains(&json!(s))))
                || filter.pinned.is_some_and(|p| p != pinned)
                || filter.min_tokens.is_some_and(|n| usage.total < n)
                || filter.max_tokens.is_some_and(|n| usage.total > n)
                || filter.min_cost.is_some_and(|n| usage.cost < n)
                || filter.max_cost.is_some_and(|n| usage.cost > n)
            {
                continue;
            }
            if let Some(search) = filter.search.as_ref().filter(|s| !s.is_empty()) {
                let hay = format!(
                    "{} {} {} {} {} {} {} {}",
                    m.id,
                    name,
                    m.project_path,
                    project_name,
                    profile,
                    m.provider,
                    annotation["notes"],
                    tags
                )
                .to_lowercase();
                if !hay.contains(&search.to_lowercase()) {
                    continue;
                }
            }
            let mut models: Vec<_> = events.iter().map(|e| e.model.clone()).collect();
            models.sort();
            models.dedup();
            let mut descendants = HashSet::new();
            let mut todo = vec![m.id.clone()];
            while let Some(parent) = todo.pop() {
                for child in children.get(&parent).into_iter().flatten() {
                    if child != &m.id && descendants.insert(child.clone()) {
                        todo.push(child.clone());
                    }
                }
            }
            let mut combined = usage.clone();
            for id in &descendants {
                if let Some(es) = own.get(id) {
                    combined.add(&total(es.iter().copied()));
                }
            }
            let elapsed = date(&m.last_at)
                .zip(date(&m.first_at))
                .map(|(a, b)| (a - b).num_seconds().max(0))
                .unwrap_or(0);
            let end = date(&m.last_at).unwrap_or(as_of);
            let mut spark = vec![0u64; 20];
            for e in &events {
                if let Some(t) = date(&e.timestamp) {
                    let seconds = (end - t).num_seconds();
                    if (0..3600).contains(&seconds) {
                        spark[19 - (seconds / 180) as usize] += e.usage.total;
                    }
                }
            }
            let mut warnings = m.warnings.clone();
            warnings.extend(events.iter().flat_map(|e| e.warnings.clone()));
            warnings.sort();
            warnings.dedup();
            sessions.push(json!({"id":m.id,"parent_id":m.parent_id,"name":name,"project":project_name,"project_path":m.project_path,"provider":m.provider,"profile":profile,"models":models,"state":state,"first_at":m.first_at,"last_at":m.last_at,"usage":usage,"combined_usage":combined,"subagents":descendants.len(),"pinned":pinned,"tags":tags,"notes":annotation["notes"].as_str().unwrap_or(""),"sparkline":spark,"source_path":m.source_path,"elapsed_seconds":elapsed,"active_seconds":activity(&events),"warnings":warnings}));
        }
        match filter.sort.as_deref().unwrap_or("recent") {
            "tokens" | "tokens_desc" => sessions.sort_by(|a, b| {
                b["usage"]["total"]
                    .as_u64()
                    .cmp(&a["usage"]["total"].as_u64())
            }),
            "cost" | "cost_desc" => sessions.sort_by(|a, b| {
                b["usage"]["cost"]
                    .as_f64()
                    .partial_cmp(&a["usage"]["cost"].as_f64())
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            "name" => sessions.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str())),
            "oldest" => {
                sessions.sort_by(|a, b| a["first_at"].as_str().cmp(&b["first_at"].as_str()))
            }
            _ => sessions.sort_by(|a, b| {
                b["pinned"]
                    .as_bool()
                    .cmp(&a["pinned"].as_bool())
                    .then(b["last_at"].as_str().cmp(&a["last_at"].as_str()))
            }),
        }
        let ids: HashSet<_> = sessions
            .iter()
            .filter_map(|s| s["id"].as_str().map(str::to_owned))
            .collect();
        let events = filtered
            .into_iter()
            .filter(|e| ids.contains(&e.session_id))
            .collect();
        Ok(View {
            sessions,
            events,
            all,
            meta,
            tz,
            range,
        })
    }
    pub(crate) fn snapshot(&self, filter: Filter) -> Result<Value> {
        let v = self.view(&filter)?;
        let as_of = v.range.as_of;
        let now_local = as_of.with_timezone(&v.tz);
        let day = now_local.format("%Y-%m-%d").to_string();
        let today = total(v.events.iter().filter(|e| {
            date(&e.timestamp).is_some_and(|t| {
                t <= as_of && t.with_timezone(&v.tz).format("%Y-%m-%d").to_string() == day
            })
        }));
        let recent = total(v.events.iter().filter(|e| {
            date(&e.timestamp).is_some_and(|t| t <= as_of && as_of - t <= Duration::minutes(5))
        }));
        let daily = daily_buckets(&v.events, v.tz, &v.range);
        let models = buckets(v.events.iter(), |e| e.model.clone());
        let providers = buckets(v.events.iter(), |e| {
            v.meta
                .get(&e.session_id)
                .map(|m| m.provider.clone())
                .unwrap_or_default()
        });
        let hours = buckets(v.events.iter(), |e| {
            date(&e.timestamp)
                .unwrap()
                .with_timezone(&v.tz)
                .format("%H:00")
                .to_string()
        });
        let weekdays = buckets(v.events.iter(), |e| {
            format!(
                "{} {}",
                date(&e.timestamp)
                    .unwrap()
                    .with_timezone(&v.tz)
                    .weekday()
                    .num_days_from_monday(),
                date(&e.timestamp)
                    .unwrap()
                    .with_timezone(&v.tz)
                    .format("%a")
            )
        });
        let prior_usage = |end: Option<DateTime<Utc>>| -> Result<Option<Usage>> {
            if let Some((start, end)) = v.range.previous_from.zip(end) {
                let mut f = filter.clone();
                f.period = None;
                f.from = Some(start.to_rfc3339());
                f.to = Some(end.to_rfc3339());
                if start == end {
                    return Ok(Some(Usage::default()));
                }
                Ok(Some(total(self.view(&f)?.events.iter())))
            } else {
                Ok(None)
            }
        };
        let previous = prior_usage(v.range.previous_to)?;
        let previous_complete = prior_usage(v.range.prior_complete_to)?;
        let mut top_sessions = v.sessions.clone();
        top_sessions.sort_by(|a, b| {
            b["usage"]["total"]
                .as_u64()
                .cmp(&a["usage"]["total"].as_u64())
                .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
        });
        top_sessions.truncate(5);
        let mut reporting = serde_json::to_value(&v.range)?;
        reporting["observed_from"] =
            json!(v.events.iter().filter_map(|e| date(&e.timestamp)).min());
        reporting["observed_to"] = json!(v.events.iter().filter_map(|e| date(&e.timestamp)).max());
        let projects = self.project_views(&v)?;
        let budgets = self.budget_views(&v.all, &v.meta, v.tz)?;
        let mut alerts = self.list("alerts")?;
        alerts.sort_by(|a, b| b["at"].as_str().cmp(&a["at"].as_str()));
        alerts.truncate(100);
        let offset = filter.offset.unwrap_or(0);
        let limit = filter.limit.unwrap_or(500).clamp(1, 500);
        let sessions: Vec<_> = v
            .sessions
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        let warnings=vec!["Observed local usage may not cover all devices or account activity.","API-equivalent cost is an estimate, not a subscription charge.","Active time sums gaps between observed events, capping each gap at five minutes; it is not human working time."];
        Ok(
            json!({"sessions":sessions,"total_sessions":v.sessions.len(),"totals":total(v.events.iter()),"today":today,"recent_rate":recent.total as f64/5.,"active_sessions":v.sessions.iter().filter(|s|s["state"]=="active").count(),"sources":self.list("sources")?,"projects":projects,"daily":daily,"models":models,"providers":providers,"hours":hours,"weekdays":weekdays,"previous":previous,"previous_complete":previous_complete,"reporting":reporting,"top_sessions":top_sessions,"budgets":budgets,"alerts":alerts,"prices":self.list("prices")?,"settings":self.settings()?,"limits":self.list("limits")?,"indexed_at":self.get("config","indexed_at")?.unwrap_or(json!(now())),"warnings":warnings,"demo":self.is_demo}),
        )
    }
    fn project_views(&self, v: &View) -> Result<Vec<Value>> {
        let configs = self.list("projects")?;
        let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
        for s in &v.sessions {
            let path = s["project_path"].as_str().unwrap_or("");
            let conf = configs.iter().find(|p| {
                p["path"] == path
                    || p["aliases"]
                        .as_array()
                        .is_some_and(|a| a.contains(&json!(path)))
            });
            let key = conf.and_then(|c| c["path"].as_str()).unwrap_or(path);
            groups.entry(key.into()).or_default().push(s);
        }
        let mut result = vec![];
        for (path, sessions) in groups {
            let conf = configs.iter().find(|p| p["path"] == path);
            let ids: HashSet<_> = sessions.iter().filter_map(|s| s["id"].as_str()).collect();
            let events: Vec<_> = v
                .events
                .iter()
                .filter(|e| ids.contains(e.session_id.as_str()))
                .collect();
            let usage = total(events.iter().copied());
            let mut models: Vec<_> = events.iter().map(|e| e.model.clone()).collect();
            models.sort();
            models.dedup();
            let mut spark = vec![0u64; 30];
            let today = v.range.as_of.with_timezone(&v.tz).date_naive();
            for e in &events {
                if let Some(t) = date(&e.timestamp) {
                    let days = (today - t.with_timezone(&v.tz).date_naive()).num_days();
                    if (0..30).contains(&days) {
                        spark[29 - days as usize] += e.usage.total;
                    }
                }
            }
            result.push(json!({"path":path,"name":conf.and_then(|p|p["name"].as_str()).filter(|s|!s.is_empty()).map(str::to_owned).unwrap_or_else(||display_path(&path)),"color":conf.and_then(|p|p["color"].as_str()).unwrap_or("#d2a56d"),"notes":conf.and_then(|p|p["notes"].as_str()).unwrap_or(""),"favorite":conf.is_some_and(|p|p["favorite"]==true),"aliases":conf.map(|p|p["aliases"].clone()).unwrap_or(json!([])),"usage":usage,"sessions":sessions.len(),"models":models,"sparkline":spark}));
        }
        result.sort_by(|a, b| {
            b["usage"]["total"]
                .as_u64()
                .cmp(&a["usage"]["total"].as_u64())
        });
        Ok(result)
    }
    pub(crate) fn detail(&self, id: &str, filter: Filter, event_query: &Value) -> Result<Value> {
        let mut broad = filter.clone();
        broad.search = None;
        broad.min_tokens = None;
        broad.max_tokens = None;
        broad.min_cost = None;
        broad.max_cost = None;
        broad.pinned = None;
        broad.offset = None;
        broad.limit = None;
        broad.state = None;
        broad.tag = None;
        let v = self.view(&broad)?;
        let session = v
            .sessions
            .iter()
            .find(|s| s["id"] == id)
            .cloned()
            .context("Session is unavailable under the current filters")?;
        let events: Vec<_> = v
            .events
            .iter()
            .filter(|e| e.session_id == id && e.scope == "session-own")
            .cloned()
            .collect();
        let count = events.len();
        let models = buckets(events.iter(), |e| e.model.clone());
        let parent = session["parent_id"].as_str();
        let relationships: Vec<_> = v
            .sessions
            .iter()
            .filter(|s| s["parent_id"] == id || parent.is_some_and(|p| s["id"] == p))
            .cloned()
            .collect();
        let mut markers = vec![];
        let mut query = self
            .db
            .prepare("SELECT data FROM markers WHERE session_id=?1 ORDER BY id")?;
        for item in query.query_map([id], |r| r.get::<_, String>(0))? {
            markers.push(serde_json::from_str::<Value>(&item?)?);
        }
        let mut last_model = "";
        for e in &events {
            if e.model != last_model {
                markers.push(json!({"timestamp":e.timestamp,"kind":"model","label":format!("Model: {}",e.model)}));
                last_model = &e.model;
            }
        }
        markers.sort_by(|a, b| a["timestamp"].as_str().cmp(&b["timestamp"].as_str()));
        let m = v.meta.get(id).context("Missing session metadata")?;
        let mut carry = Usage::default();
        for e in v
            .all
            .iter()
            .filter(|e| e.session_id == id && e.scope == "carry-in")
        {
            carry.add(&e.usage);
        }
        let durations: Vec<_> = events.iter().filter_map(|e| e.duration_ms).collect();
        let coverage=vec![format!("{} canonical usage events in selected range; event table is searchable and paginated",count),"Input excludes cache categories; reasoning is a subset of output and is not added to the total.".into(),"Historical carry-in and parent rollups are excluded from observed totals.".into(),"Prompt bodies and tool arguments are not indexed.".into()];
        let search = event_query["event_search"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
        let offset = event_query["event_offset"]
            .as_u64()
            .unwrap_or(0)
            .min(usize::MAX as u64) as usize;
        let matches: Vec<_> = events
            .iter()
            .rev()
            .filter(|e| {
                search.is_empty()
                    || format!("{} {} {} {}", e.id, e.model, e.scope, e.kind)
                        .to_lowercase()
                        .contains(&search)
            })
            .collect();
        let matching = matches.len();
        let rows: Vec<_> = matches
            .into_iter()
            .skip(offset)
            .take(200)
            .cloned()
            .collect();
        Ok(
            json!({"session":session,"events":rows,"event_count":count,"event_matches":matching,"event_offset":offset,"timeline":timeline(&events,v.tz,None),"models":models,"relationships":relationships,"markers":markers,"context":{"used":m.context_used,"capacity":m.context_capacity,"observed_at":m.context_at},"carry_in":carry,"request_duration_ms":if durations.is_empty(){None}else{Some(durations.iter().sum::<u64>())},"coverage":coverage}),
        )
    }
    fn budget_views(
        &self,
        events: &[Event],
        meta: &HashMap<String, SessionMeta>,
        tz: Tz,
    ) -> Result<Vec<Value>> {
        let current = Utc::now();
        let local = current.with_timezone(&tz);
        let mut out = vec![];
        for mut budget in self.list("budgets")? {
            let period = budget["period"].as_str().unwrap_or("month").to_owned();
            let window_minutes = if period == "5h" {
                300
            } else {
                budget["window_minutes"]
                    .as_i64()
                    .unwrap_or(300)
                    .clamp(1, 525600)
            };
            let start = match period.as_str() {
                "day" => bound(Some(&local.format("%Y-%m-%d").to_string()), tz, false)?.unwrap(),
                "5h" => current - Duration::hours(5),
                "window" => current - Duration::minutes(window_minutes),
                _ => bound(
                    Some(&format!("{}-{:02}-01", local.year(), local.month())),
                    tz,
                    false,
                )?
                .unwrap(),
            };
            let project = budget["project"].as_str().filter(|x| !x.is_empty());
            let configs = self.list("projects")?;
            let chosen: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.scope == "session-own"
                        && date(&e.timestamp).is_some_and(|t| t >= start && t <= current)
                        && project.is_none_or(|p| {
                            meta.get(&e.session_id).is_some_and(|m| {
                                m.project_path == p
                                    || configs.iter().any(|c| {
                                        c["path"] == p
                                            && c["aliases"]
                                                .as_array()
                                                .is_some_and(|a| a.contains(&json!(m.project_path)))
                                    })
                            })
                        })
                })
                .collect();
            let usage = total(chosen.iter().copied());
            let used = if budget["unit"] == "tokens" {
                usage.total as f64
            } else {
                usage.cost
            };
            let amount = budget["amount"].as_f64().unwrap_or(1.);
            let percentage = used / amount * 100.;
            budget["used"] = json!(used);
            budget["percentage"] = json!(percentage);
            budget["unpriced_tokens"] = json!(usage.unpriced_tokens);
            budget["unknown_fields"] = json!(usage.unknown_fields);
            // Keep known usage as a lower bound, but do not forecast from an
            // incomplete window. Missing reasoning does not change token totals
            // because reasoning is already included in the output category.
            let incomplete = if budget["unit"] == "tokens" {
                usage
                    .unknown_fields
                    .iter()
                    .any(|field| field != "reasoning")
            } else {
                usage.unpriced_tokens > 0 || !usage.unknown_fields.is_empty()
            };
            let recent: Vec<_> = chosen
                .iter()
                .copied()
                .filter(|e| {
                    date(&e.timestamp).is_some_and(|t| current - t <= Duration::minutes(30))
                })
                .collect();
            let u = total(recent.iter().copied());
            let rate = if budget["unit"] == "tokens" {
                u.total as f64 / 30.
            } else {
                u.cost / 30.
            };
            let threshold = budget["threshold"].as_f64().unwrap_or(80.);
            let target = amount * threshold / 100.;
            let fresh = recent
                .last()
                .and_then(|e| date(&e.timestamp))
                .is_some_and(|t| current - t < Duration::minutes(10));
            let projected =
                if !incomplete && recent.len() >= 3 && fresh && rate > 0. && target > used {
                    let minutes = (target - used) / rate;
                    if minutes.is_finite() && minutes < 525600. {
                        Some((current + Duration::seconds((minutes * 60.) as i64)).to_rfc3339())
                    } else {
                        None
                    }
                } else {
                    None
                };
            budget["projected_at"] = json!(projected);
            budget["sample_minutes"] = json!(30);
            if percentage >= threshold {
                let rolling = period == "5h" || period == "window";
                let window = if rolling {
                    format!("rolling:{}", current.timestamp() / (window_minutes * 60))
                } else {
                    start.to_rfc3339()
                };
                let alert_id = hash(&format!("{}:{}:{}", budget["id"], window, threshold));
                let cooled_down = !rolling
                    || !self.list("alerts")?.iter().any(|a| {
                        a["budget_id"] == budget["id"]
                            && a["at"]
                                .as_str()
                                .and_then(date)
                                .is_some_and(|t| current - t < Duration::minutes(window_minutes))
                    });
                if cooled_down && self.get("alerts", &alert_id)?.is_none() {
                    self.put("alerts",&alert_id,&json!({"id":alert_id,"budget_id":budget["id"],"message":format!("{} reached {:.0}% of its local budget",budget["name"].as_str().unwrap_or("Budget"),percentage),"at":now()}))?;
                }
            }
            out.push(budget);
        }
        Ok(out)
    }
    pub(crate) fn compare(&self, args: &Value) -> Result<Value> {
        let filter: Filter =
            serde_json::from_value(args.get("filter").cloned().unwrap_or(json!({})))?;
        let mut items = vec![];
        if let Some(ranges) = args["ranges"].as_array() {
            if ranges.len() != 2 {
                bail!("Choose exactly two date ranges");
            }
            for range in ranges {
                let mut f = filter.clone();
                f.period = None;
                f.from = Some(required(range, "from")?.into());
                f.to = Some(required(range, "to")?.into());
                let v = self.view(&f)?;
                let refs: Vec<_> = v
                    .events
                    .iter()
                    .filter(|e| e.scope == "session-own")
                    .collect();
                let elapsed = refs
                    .first()
                    .and_then(|a| date(&a.timestamp))
                    .zip(refs.last().and_then(|b| date(&b.timestamp)))
                    .map(|(a, b)| (b - a).num_seconds())
                    .unwrap_or(0);
                items.push(json!({"label":format!("{} → {}",f.from.unwrap_or_default(),f.to.unwrap_or_default()),"usage":total(v.events.iter()),"elapsed_seconds":elapsed,"active_seconds":v.sessions.iter().filter_map(|s|s["active_seconds"].as_i64()).sum::<i64>(),"models":buckets(v.events.iter(),|e|e.model.clone()).iter().map(|b|b["key"].clone()).collect::<Vec<_>>(),"timeline":timeline(&v.events,v.tz,if args["alignment"]=="elapsed"{refs.first().and_then(|e|date(&e.timestamp))}else{None})}));
            }
        } else if let Some(ids) = args["ids"].as_array() {
            if ids.len() > 4 {
                bail!("Compare up to four sessions");
            }
            let v = self.view(&filter)?;
            for id in ids {
                if let Some(s) = v.sessions.iter().find(|s| s["id"] == *id) {
                    let es: Vec<_> = v
                        .events
                        .iter()
                        .filter(|e| json!(e.session_id) == *id && e.scope == "session-own")
                        .cloned()
                        .collect();
                    items.push(json!({"id":s["id"],"label":s["name"],"usage":s["usage"],"elapsed_seconds":s["elapsed_seconds"],"active_seconds":s["active_seconds"],"models":s["models"],"timeline":timeline(&es,v.tz,if args["alignment"]=="elapsed"{date(s["first_at"].as_str().unwrap_or(""))}else{None})}));
                }
            }
        }
        Ok(json!({"items":items}))
    }
    pub(crate) fn export(&self, args: &Value) -> Result<Value> {
        let filter: Filter =
            serde_json::from_value(args.get("filter").cloned().unwrap_or(json!({})))?;
        let v = self.view(&filter)?;
        let ids = args["session_ids"].as_array();
        let sessions: Vec<_> = v
            .sessions
            .iter()
            .filter(|s| ids.is_none_or(|ids| ids.contains(&s["id"])))
            .cloned()
            .collect();
        let selected: HashSet<_> = sessions.iter().filter_map(|s| s["id"].as_str()).collect();
        let events: Vec<_> = v
            .events
            .iter()
            .filter(|e| selected.contains(e.session_id.as_str()) && e.scope == "session-own")
            .cloned()
            .collect();
        let paths = args["redact_paths"] == true;
        let labels = args["redact_labels"] == true;
        let project_view = View {
            sessions: sessions.clone(),
            events: events.clone(),
            all: vec![],
            meta: HashMap::new(),
            tz: v.tz,
            range: v.range.clone(),
        };
        let history: Vec<_> = self
            .list("config")?
            .into_iter()
            .filter(|p| p.get("model").is_some() && p.get("version").is_some())
            .collect();
        let mut data = json!({"schema_version":1,"projects":self.project_views(&project_view)?,"pricing_history":history,"exported_at":now(),"units":{"tokens":"integer tokens","cost":"USD API-equivalent estimate"},"timezone":v.tz.to_string(),"reporting":v.range,"filters":clean_filter(&filter),"pricing":self.list("prices")?,"coverage":"Local observed records; carry-in and parent rollups excluded from totals; not complete account usage.","totals":total(events.iter()),"sessions":sessions,"events":events});
        redact(&mut data, paths, labels);
        let format = required(args, "format")?;
        let (content, mime, extension) = match format {
            "json" => (
                serde_json::to_string_pretty(&data)?,
                "application/json",
                "json",
            ),
            "csv" => {
                let header = [
                    "session_id",
                    "timestamp_utc",
                    "model",
                    "input_tokens_uncached",
                    "output_tokens_including_reasoning",
                    "cache_read_tokens",
                    "cache_write_tokens",
                    "reasoning_tokens_subset",
                    "total_tokens",
                    "known_cost_usd",
                    "unpriced_tokens",
                    "scope",
                    "pricing_version",
                    "source_path",
                    "timezone",
                    "coverage",
                    "filters_json",
                    "pricing_history_json",
                    "unknown_categories_json",
                    "inferred_price_tokens",
                    "reporting_range_json",
                ];
                let mut csv = header.join(",") + "\r\n";
                for e in data["events"].as_array().unwrap() {
                    let vals = vec![
                        e["session_id"].clone(),
                        e["timestamp"].clone(),
                        e["model"].clone(),
                        e["usage"]["input"].clone(),
                        e["usage"]["output"].clone(),
                        e["usage"]["cache_read"].clone(),
                        e["usage"]["cache_write"].clone(),
                        e["usage"]["reasoning"].clone(),
                        e["usage"]["total"].clone(),
                        e["usage"]["cost"].clone(),
                        e["usage"]["unpriced_tokens"].clone(),
                        e["scope"].clone(),
                        e["pricing_version"].clone(),
                        e["source_path"].clone(),
                        json!(v.tz.to_string()),
                        data["coverage"].clone(),
                        data["filters"].clone(),
                        data["pricing_history"].clone(),
                        e["usage"]["unknown_fields"].clone(),
                        e["usage"]["inferred_price_tokens"].clone(),
                        data["reporting"].clone(),
                    ];
                    csv.push_str(&vals.iter().map(csv_cell).collect::<Vec<_>>().join(","));
                    csv.push_str("\r\n");
                }
                (csv, "text/csv", "csv")
            }
            _ => bail!("Choose CSV or JSON export"),
        };
        Ok(
            json!({"content":content,"filename":format!("chip-count-{}.{}",Utc::now().format("%Y-%m-%d"),extension),"mime":mime}),
        )
    }
}
fn redact(v: &mut Value, paths: bool, labels: bool) {
    match v {
        Value::Object(map) => {
            for (k, value) in map {
                if (paths
                    && ["source_path", "project_path", "path", "aliases"].contains(&k.as_str()))
                    || (labels
                        && ["name", "notes", "tags", "profile", "label", "search", "tag"]
                            .contains(&k.as_str()))
                    || ((paths || labels) && k == "project")
                {
                    *value = if value.is_array() {
                        json!([])
                    } else {
                        json!("[redacted]")
                    };
                } else {
                    redact(value, paths, labels);
                }
            }
        }
        Value::Array(a) => {
            for value in a {
                redact(value, paths, labels)
            }
        }
        _ => {}
    }
}
fn csv_cell(v: &Value) -> String {
    let raw = v.as_str().map(str::to_owned).unwrap_or_else(|| {
        if v.is_null() {
            String::new()
        } else {
            v.to_string()
        }
    });
    let safe = if raw.trim_start().starts_with(['=', '+', '-', '@'])
        || raw.starts_with(['\t', '\r', '\n'])
    {
        format!("'{raw}")
    } else {
        raw
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
