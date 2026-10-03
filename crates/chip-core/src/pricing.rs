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
        ];
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
                version: "bundled-2026-10-03".into(),
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
            if e.warnings
                .iter()
                .any(|w| w.starts_with("Pricing unavailable:"))
                && !p.overridden
            {
                e.usage.unpriced_tokens = e.usage.total;
                e.pricing_version = Some("unpriced".into());
                return Ok(());
            }
            // Preserve nine decimal places of per-million rates, then round once per event.
            // This avoids losing sub-nanodollar per-token rates in small local overrides.
            let calc = |tokens: u64, rate: f64| -> i128 {
                tokens as i128 * (rate * 1_000_000_000.).round() as i128
            };
            let n = calc(e.usage.input, p.input)
                + calc(e.usage.output, p.output)
                + calc(e.usage.cache_read, p.cache_read)
                + calc(e.usage.cache_write, p.cache_write);
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
