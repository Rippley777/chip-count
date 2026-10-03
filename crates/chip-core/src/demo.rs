use crate::{model::*, Engine};
use anyhow::Result;
use chrono::{Duration, Utc};
use serde_json::json;
impl Engine {
    fn demo_metadata(&self, table: &str, id: &str, value: &serde_json::Value) -> Result<()> {
        if self.get("config", "demo_seeded")?.is_none() {
            self.put(table, id, value)?;
        }
        Ok(())
    }
    pub(crate) fn populate_demo(&mut self) -> Result<()> {
        let now = Utc::now();
        let projects = [
            ("/demo/rippley/chip-count", "Chip Count", "#d2a56d"),
            ("/demo/rippley/pit-boss", "Pit Boss", "#8bc6b1"),
            ("/demo/rippley/deck", "Deck", "#8caad7"),
            ("/demo/rippley/house-edge", "House Edge", "#bd9ed7"),
            ("/demo/research/observatory", "Observatory", "#d99b8c"),
        ];
        for provider in ["claude", "codex"] {
            self.demo_metadata("sources",provider,&json!({"id":provider,"provider":provider,"label":if provider=="claude"{"Personal"}else{"Work"},"path":format!("/demo/{provider}"),"enabled":true,"exclusions":[],"status":"healthy","files":72,"recognized":1800,"ignored":430,"warnings":0,"last_read":now.to_rfc3339(),"last_activity":now.to_rfc3339(),"message":"Isolated demonstration dataset — no local logs are included"}))?;
        }
        for (path, name, color) in projects {
            self.demo_metadata("projects",path,&json!({"path":path,"name":name,"color":color,"notes":format!("{} development workspace",name),"favorite":name=="Chip Count","aliases":[]}))?;
        }
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            for day in 0..30i64 {
                for slot in 0..4i64 {
                    let provider = if (day + slot) % 2 == 0 {
                        "claude"
                    } else {
                        "codex"
                    };
                    let project = projects[((day + slot) % 5) as usize];
                    let id = format!("{provider}:demo-{day:02}-{slot}");
                    let first = if day == 0 {
                        now - Duration::minutes(58 + slot * 13)
                    } else {
                        now - Duration::days(day) - Duration::hours(slot * 2)
                    };
                    let count = if day == 0 { 18 } else { 8 + (day + slot) % 16 };
                    let last = if day == 0 && slot < 3 {
                        now - Duration::seconds(15 + slot * 42)
                    } else {
                        first + Duration::minutes((count - 1) * 3)
                    };
                    let meta = SessionMeta {
                        id: id.clone(),
                        raw_id: format!("demo-{day:02}-{slot}"),
                        provider: provider.into(),
                        parent_id: None,
                        project_path: project.0.into(),
                        first_at: first.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        last_at: last.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        source_path: format!("/demo/{provider}/{day}-{slot}.jsonl"),
                        completed: day > 0 && (day + slot) % 3 == 0,
                        context_used: if day == 0 {
                            Some(42800 + slot as u64 * 13270)
                        } else {
                            None
                        },
                        context_capacity: if day == 0 {
                            Some(if provider == "claude" { 200000 } else { 272000 })
                        } else {
                            None
                        },
                        context_at: if day == 0 {
                            Some(last.to_rfc3339())
                        } else {
                            None
                        },
                        warnings: vec![],
                    };
                    self.merge_session(&meta)?;
                    let names = [
                        "Refine live session workspace",
                        "Build token accounting pipeline",
                        "Investigate cache behavior",
                        "Add project budget alerts",
                        "Polish session inspector",
                        "Review ingestion fixtures",
                    ];
                    self.demo_metadata("annotations",&id,&json!({"alias":names[((day*4+slot)%names.len() as i64) as usize],"notes":if day==0&&slot==0{"Investigating a usage spike after a model switch. Compare own usage with the background agent."}else{""},"tags":if slot==0{vec!["feature"]}else if slot==1{vec!["research"]}else{vec![]},"pinned":day==0&&slot<2}))?;
                    for index in 0..count {
                        let factor = 1 + ((day * 17 + slot * 13 + index * 7) % 9) as u64;
                        let input = 1200 + factor * 610;
                        let output = 230 + factor * 195;
                        let cache = if provider == "claude" {
                            6000 + factor * 8100
                        } else {
                            2000 + factor * 5100
                        };
                        let write = if provider == "claude" && index % 5 == 0 {
                            2200 + factor * 1200
                        } else {
                            0
                        };
                        let model = if provider == "claude" {
                            if index > count / 2 && slot == 0 {
                                "claude-opus-4-6"
                            } else if slot == 3 {
                                "claude-haiku-4-5"
                            } else {
                                "claude-sonnet-4-6"
                            }
                        } else if slot == 3 && day % 3 == 0 {
                            "gpt-experimental"
                        } else {
                            "gpt-5.3-codex"
                        };
                        let ts = first
                            + Duration::milliseconds(
                                (last - first).num_milliseconds() * index / (count - 1).max(1),
                            );
                        let mut e = Event {
                            id: hash(&format!("demo:{id}:{index}")),
                            session_id: id.clone(),
                            timestamp: ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                            ingested_at: now.to_rfc3339(),
                            model: model.into(),
                            usage: Usage::tokens(
                                input,
                                output,
                                cache,
                                write,
                                if provider == "codex" { output / 3 } else { 0 },
                            ),
                            scope: "session-own".into(),
                            kind: if provider == "codex" {
                                "cumulative-difference"
                            } else {
                                "response-final"
                            }
                            .into(),
                            request_id: Some(format!("req-{day}-{slot}-{index}")),
                            turn_id: Some(format!("turn-{index}")),
                            source_path: meta.source_path.clone(),
                            source_line: (index + 1) as u64,
                            parser_version: PARSER.into(),
                            pricing_version: None,
                            reported: provider == "claude",
                            warnings: vec![],
                            duration_ms: if index % 3 == 0 {
                                Some(3800 + factor * 1300)
                            } else {
                                None
                            },
                        };
                        self.insert_event(&mut e, provider, &meta.source_path)?;
                    }
                    if slot == 0 && day % 3 == 0 {
                        let child_id = format!("{id}/agent:review");
                        let child = SessionMeta {
                            id: child_id.clone(),
                            raw_id: "review-agent".into(),
                            parent_id: Some(id.clone()),
                            first_at: (first + Duration::minutes(12))
                                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                            source_path: format!("/demo/{provider}/{day}-{slot}-agent.jsonl"),
                            ..meta.clone()
                        };
                        self.merge_session(&child)?;
                        self.demo_metadata("annotations",&child_id,&json!({"alias":"Review accounting edge cases","tags":["subagent"],"notes":"Background review agent","pinned":false}))?;
                        for i in 0..5 {
                            let ts = first + Duration::minutes(12 + i * 4);
                            let mut e = Event {
                                id: hash(&format!("{child_id}:{i}")),
                                session_id: child_id.clone(),
                                timestamp: ts.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                                ingested_at: now.to_rfc3339(),
                                model: if provider == "claude" {
                                    "claude-sonnet-4-6"
                                } else {
                                    "gpt-5.3-codex"
                                }
                                .into(),
                                usage: Usage::tokens(2000 + i as u64 * 800, 900, 14000, 0, 0),
                                scope: "session-own".into(),
                                kind: "response-final".into(),
                                request_id: Some(format!("agent-{i}")),
                                turn_id: None,
                                source_path: child.source_path.clone(),
                                source_line: i as u64 + 1,
                                parser_version: PARSER.into(),
                                pricing_version: None,
                                reported: true,
                                warnings: vec![],
                                duration_ms: None,
                            };
                            self.insert_event(&mut e, provider, &child.source_path)?;
                        }
                    }
                    if day == 0 && slot == 0 {
                        let marker = json!({"timestamp":(first+Duration::minutes(35)).to_rfc3339(),"kind":"compaction","label":"Context compacted"});
                        self.db.execute(
                            "INSERT OR REPLACE INTO markers(id,session_id,data) VALUES(?1,?2,?3)",
                            rusqlite::params!["demo-compaction", id, marker.to_string()],
                        )?;
                    }
                }
            }
            Ok(())
        })();
        if result.is_err() {
            self.db.execute_batch("ROLLBACK")?;
        } else {
            self.db.execute_batch("COMMIT")?;
        }
        result?;
        self.demo_metadata("budgets","demo-month",&json!({"id":"demo-month","name":"Monthly workspace","amount":300.,"unit":"usd","period":"month","project":null,"threshold":80.}))?;
        self.demo_metadata("budgets","demo-day",&json!({"id":"demo-day","name":"Daily token pace","amount":5_000_000.,"unit":"tokens","period":"day","project":null,"threshold":80.}))?;
        self.demo_metadata("budgets","demo-project",&json!({"id":"demo-project","name":"Chip Count · focus window","amount":12.,"unit":"usd","period":"5h","project":"/demo/rippley/chip-count","threshold":75.}))?;
        self.put("limits","demo-limit",&json!({"provider":"codex","scope":"Demo reported primary window","used_percent":42.,"resets_at":(now+Duration::hours(3)).to_rfc3339(),"observed_at":now.to_rfc3339(),"window_minutes":300}))?;
        self.put("config", "indexed_at", &json!(now.to_rfc3339()))?;
        self.put("config", "demo_seeded", &json!(true))?;
        Ok(())
    }
}
