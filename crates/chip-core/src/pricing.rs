use crate::{model::*, required, Engine};
use anyhow::{Context, Result};
use rusqlite::params;
use serde_json::Value;
impl Engine {
    pub(crate) fn seed_prices(&self) -> Result<()> {
        let models = [
            ("claude-sonnet-4", 3., 15., 0.3, 3.75),
            ("claude-sonnet-4-5", 3., 15., 0.3, 3.75),
            ("claude-sonnet-4-6", 3., 15., 0.3, 3.75),
            ("claude-opus-4-5", 5., 25., 0.5, 6.25),
            ("claude-opus-4-6", 5., 25., 0.5, 6.25),
            ("claude-haiku-4-5", 1., 5., 0.1, 1.25),
            ("gpt-5.3-codex", 1.75, 14., 0.175, 1.75),
            ("gpt-6-astra", 10., 50., 1., 12.5),
            ("gpt-6.1-sol", 2., 10., 0.1, 2.5),
            ("gpt-6-sol", 2., 10., 0.2, 2.5),
            ("gpt-6-luna", 0.1, 0.5, 0.01, 0.125),
            ("gpt-5.6-sol", 4., 20., 0.4, 5.),
            ("gpt-5.6-terra", 2., 12., 0.2, 2.5),
        ];
        // Install new models and recover their previously missing estimates atomically.
        // Already priced events and local rate overrides keep their original versions.
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            for (model, input, output, cache_read, cache_write) in models {
                if self.get("prices", model)?.is_some() {
                    continue;
                }
                let price = Price {
                    model: model.into(),
                    input,
                    output,
                    cache_read,
                    cache_write,
                    version: if modern_openai(model) {
                        "bundled-2026-10-03-openai-v2"
                    } else {
                        "bundled-2026-10-03"
                    }
                    .into(),
                    source: if model.starts_with("claude") {
                        "https://platform.claude.com/docs/en/about-claude/pricing"
                    } else {
                        "https://developers.openai.com/api/docs/pricing"
                    }
                    .into(),
                    retrieved_at: "2026-10-03T00:00:00Z".into(),
                    overridden: false,
                };
                self.store_price(&price)?;
                self.recover_unpriced_model(model)?;
            }
            Ok(())
        })();
        self.db
            .execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        result
    }
    fn recover_unpriced_model(&self, model: &str) -> Result<()> {
        let mut statement = self.db.prepare(
            "SELECT data FROM events WHERE model=?1 AND json_extract(data,'$.pricing_version')='unpriced'",
        )?;
        let rows = statement.query_map([model], |r| r.get::<_, String>(0))?;
        let mut events = vec![];
        for row in rows {
            events.push(serde_json::from_str::<Event>(&row?)?);
        }
        drop(statement);
        for mut event in events {
            event.pricing_version = None;
            self.apply_price(&mut event)?;
            if event.pricing_version.as_deref() != Some("unpriced") {
                self.db.execute(
                    "UPDATE events SET data=?1,nano=?2 WHERE id=?3",
                    params![serde_json::to_string(&event)?, event.usage.nano, event.id],
                )?;
            }
        }
        Ok(())
    }
    fn store_price(&self, p: &Price) -> Result<()> {
        let v = serde_json::to_value(p)?;
        self.put("prices", &p.model, &v)?;
        self.put("config", &format!("price:{}:{}", p.model, p.version), &v)
    }
    pub(crate) fn save_price(&self, v: &Value) -> Result<()> {
        let read = |name: &str| -> Result<f64> {
            v[name]
                .as_f64()
                .filter(|x| x.is_finite() && *x >= 0.0 && *x <= 1_000_000.0)
                .with_context(|| {
                    format!("{name} must be a finite nonnegative price per million tokens")
                })
        };
        self.store_price(&Price {
            model: required(v, "model")?.into(),
            input: read("input")?,
            output: read("output")?,
            cache_read: read("cache_read")?,
            cache_write: read("cache_write")?,
            version: format!("override-{}-{}", now(), &hash(&v.to_string())[..8]),
            source: "Local override · USD per million tokens".into(),
            retrieved_at: now(),
            overridden: true,
        })
    }
    pub(crate) fn apply_price(&self, e: &mut Event) -> Result<()> {
        let alias = pricing_alias(&e.model);
        let price = if let Some(version) = &e.pricing_version {
            if version == "unpriced" {
                None
            } else {
                self.get("config", &format!("price:{}:{version}", e.model))?
                    .or(self.get("config", &format!("price:{alias}:{version}"))?)
            }
        } else {
            self.get("prices", &e.model)?.or(self.get("prices", alias)?)
        };
        e.usage.nano = 0;
        e.usage.cost = 0.;
        e.usage.events = if e.scope == "session-own" { 1 } else { 0 };
        e.usage.unpriced_tokens = 0;
        if let Some(price) = price {
            let p: Price = serde_json::from_value(price)?;
            let long_context = modern_openai(&p.model)
                && e.usage
                    .input
                    .saturating_add(e.usage.cache_read)
                    .saturating_add(e.usage.cache_write)
                    > 272_000
                && !p.overridden;
            if e.warnings
                .iter()
                .any(|w| w.starts_with("Pricing unavailable:")
                    && !(modern_openai(&p.model) && w == "Pricing unavailable: long-context tier requires a verified rate override"))
                && !p.overridden
            {
                e.usage.unpriced_tokens = e.usage.total;
                e.pricing_version = Some("unpriced".into());
                return Ok(());
            }
            e.warnings.retain(|w| w != "Pricing basis: long-context request (>272K input); 2x input/cache and 1.5x output rates");
            if modern_openai(&p.model) && !p.overridden {
                e.warnings.retain(|w| {
                    w != "Pricing unavailable: long-context tier requires a verified rate override"
                });
                if long_context {
                    let basis = "Pricing basis: long-context request (>272K input); 2x input/cache and 1.5x output rates";
                    if !e.warnings.iter().any(|w| w == basis) {
                        e.warnings.push(basis.into());
                    }
                }
            }
            // Preserve nine decimal places of per-million rates, then round once per event.
            // This avoids losing sub-nanodollar per-token rates in small local overrides.
            let calc = |tokens: u64, rate: f64| -> i128 {
                tokens as i128 * (rate * 1_000_000_000.).round() as i128
            };
            let input_multiplier = if long_context { 2. } else { 1. };
            let output_multiplier = if long_context { 1.5 } else { 1. };
            let n = calc(e.usage.input, p.input * input_multiplier)
                + calc(e.usage.output, p.output * output_multiplier)
                + calc(e.usage.cache_read, p.cache_read * input_multiplier)
                + calc(e.usage.cache_write, p.cache_write * input_multiplier);
            e.usage.nano = ((n + 500_000) / 1_000_000).min(i64::MAX as i128) as i64;
            e.usage.cost = e.usage.nano as f64 / 1e9;
            e.pricing_version = Some(p.version);
        } else {
            e.usage.unpriced_tokens = e.usage.total;
            e.pricing_version = Some("unpriced".into());
        }
        Ok(())
    }
    pub(crate) fn reprice(&self) -> Result<()> {
        let mut statement = self.db.prepare("SELECT data FROM events")?;
        let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
        let mut events = vec![];
        for row in rows {
            events.push(serde_json::from_str::<Event>(&row?)?);
        }
        drop(statement);
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            for mut e in events {
                e.pricing_version = None;
                self.apply_price(&mut e)?;
                self.db.execute(
                    "UPDATE events SET data=?1,nano=?2 WHERE id=?3",
                    params![serde_json::to_string(&e)?, e.usage.nano, e.id],
                )?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.db.execute_batch("ROLLBACK")?;
        } else {
            self.db.execute_batch("COMMIT")?;
        }
        result
    }
}
fn modern_openai(model: &str) -> bool {
    matches!(
        model,
        "gpt-6-astra"
            | "gpt-6.1-sol"
            | "gpt-6-sol"
            | "gpt-6-luna"
            | "gpt-5.6-sol"
            | "gpt-5.6-terra"
    )
}
fn pricing_alias(model: &str) -> &str {
    // Dated Claude snapshots share a verified family price, preserving the original event model.
    for alias in [
        "claude-sonnet-4-6",
        "claude-sonnet-4-5",
        "claude-opus-4-6",
        "claude-opus-4-5",
        "claude-haiku-4-5",
        "claude-sonnet-4",
    ] {
        if model == alias
            || model.strip_prefix(alias).is_some_and(|s| {
                s.starts_with('-')
                    && s[1..].len() == 8
                    && s[1..].bytes().all(|x| x.is_ascii_digit())
            })
        {
            return alias;
        }
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(model: &str, usage: Usage) -> Event {
        serde_json::from_value(json!({
            "id":"event", "session_id":"session", "timestamp":now(), "model":model,
            "usage":usage, "scope":"session-own", "kind":"response-record",
            "request_id":null, "turn_id":null, "source_path":"fixture.jsonl",
            "source_line":1, "parser_version":PARSER, "pricing_version":null,
            "reported":true, "warnings":[], "duration_ms":null
        }))
        .unwrap()
    }

    #[test]
    fn current_codex_models_have_verified_disjoint_category_rates() {
        let engine = Engine::demo().unwrap();
        for (model, nano) in [
            ("gpt-6-astra", 73_500_000),
            ("gpt-6.1-sol", 14_600_000),
            ("gpt-6-sol", 14_700_000),
            ("gpt-6-luna", 735_000),
            ("gpt-5.6-sol", 29_400_000),
            ("gpt-5.6-terra", 16_700_000),
        ] {
            let mut e = event(model, Usage::tokens(1000, 1000, 1000, 1000, 500));
            engine.apply_price(&mut e).unwrap();
            assert_eq!(e.usage.nano, nano, "{model}");
            assert_eq!(e.usage.total, 4000, "Reasoning is already part of output");
            assert_eq!(e.usage.unpriced_tokens, 0);
            assert_eq!(
                e.pricing_version.as_deref(),
                Some("bundled-2026-10-03-openai-v2")
            );
        }
    }

    #[test]
    fn long_context_uses_request_input_including_cache_at_the_exact_threshold() {
        let engine = Engine::demo().unwrap();
        let mut e = event("gpt-6-astra", Usage::tokens(2000, 1000, 270_000, 0, 500));
        engine.apply_price(&mut e).unwrap();
        assert_eq!(e.usage.nano, 340_000_000);
        // Input plus cache exceeds 272K by one. Output and reasoning never set the tier.
        e.usage = Usage::tokens(2001, 1000, 270_000, 0, 500);
        e.warnings.push(
            "Pricing unavailable: long-context tier requires a verified rate override".into(),
        );
        engine.apply_price(&mut e).unwrap();
        assert_eq!(e.usage.nano, 655_020_000);
        assert!(e
            .warnings
            .iter()
            .all(|w| !w.starts_with("Pricing unavailable:")));
        assert!(e.warnings.iter().any(|w| w.starts_with("Pricing basis:")));
        // A local override is the user's exact per-category assumption, without hidden multipliers.
        engine.save_price(&json!({"model":"gpt-6-astra","input":1,"output":2,"cache_read":0.1,"cache_write":1.25})).unwrap();
        e.pricing_version = None;
        engine.apply_price(&mut e).unwrap();
        assert_eq!(e.usage.nano, 31_001_000);
    }

    #[test]
    fn unknown_models_and_unverified_service_tiers_remain_explicitly_unpriced() {
        let engine = Engine::demo().unwrap();
        let mut e = event("codex-auto-review", Usage::tokens(100, 10, 0, 0, 0));
        engine.apply_price(&mut e).unwrap();
        assert_eq!(e.usage.unpriced_tokens, 110);
        e.model = "gpt-6-astra".into();
        e.pricing_version = None;
        e.warnings.push(
            "Pricing unavailable: recorded service tier unknown is not in the standard snapshot"
                .into(),
        );
        engine.apply_price(&mut e).unwrap();
        assert_eq!(e.usage.nano, 0);
        assert_eq!(e.usage.unpriced_tokens, 110);
    }
}
