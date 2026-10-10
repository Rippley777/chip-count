use crate::{model::*, Engine};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

pub const CATALOG_URL: &str = "https://models.dev/api.json";

fn rate(cost: &Value, name: &str) -> Result<f64> {
    cost[name]
        .as_f64()
        .filter(|r| r.is_finite() && *r >= 0. && *r <= 1_000_000.)
        .with_context(|| {
            format!("Catalog {name} must be a nonnegative USD rate per million tokens")
        })
}

fn catalog_prices(catalog: &Value, retrieved_at: &str) -> Result<Vec<Price>> {
    let mut prices = vec![];
    for (provider, prefix) in [("openai", "gpt-"), ("anthropic", "claude-")] {
        let models = catalog[provider]["models"]
            .as_object()
            .with_context(|| format!("Catalog is missing {provider} models"))?;
        let before = prices.len();
        for (id, model) in models {
            if !(id.starts_with(prefix) || provider == "openai" && id.starts_with('o')) {
                continue;
            }
            if !model["modalities"]["output"]
                .as_array()
                .is_some_and(|a| a == &[json!("text")])
            {
                continue;
            }
            let cost = &model["cost"];
            // Missing cache prices are unknown, never assumed to be free.
            if ["input", "output", "cache_read", "cache_write"]
                .iter()
                .any(|key| cost.get(key).is_none())
            {
                continue;
            }
            let mut tiers = vec![];
            if let Some(entries) = cost.get("tiers") {
                for entry in entries
                    .as_array()
                    .context("Catalog tiers must be an array")?
                {
                    ensure!(
                        entry["tier"]["type"] == "context",
                        "Unsupported catalog price tier"
                    );
                    tiers.push(PriceTier {
                        above_tokens: entry["tier"]["size"]
                            .as_u64()
                            .filter(|n| *n > 0)
                            .context("Invalid context tier threshold")?,
                        input: rate(entry, "input")?,
                        output: rate(entry, "output")?,
                        cache_read: rate(entry, "cache_read")?,
                        cache_write: rate(entry, "cache_write")?,
                    });
                }
            } else if cost.get("context_over_200k").is_some() {
                let entry = &cost["context_over_200k"];
                tiers.push(PriceTier {
                    above_tokens: 200_000,
                    input: rate(entry, "input")?,
                    output: rate(entry, "output")?,
                    cache_read: rate(entry, "cache_read")?,
                    cache_write: rate(entry, "cache_write")?,
                });
            }
            tiers.sort_by_key(|t| t.above_tokens);
            let mut price = Price {
                model: id.clone(),
                input: rate(cost, "input")?,
                output: rate(cost, "output")?,
                cache_read: rate(cost, "cache_read")?,
                cache_write: rate(cost, "cache_write")?,
                version: String::new(),
                source: format!("Models.dev catalog · {provider} · {CATALOG_URL}"),
                retrieved_at: retrieved_at.into(),
                overridden: false,
                inferred: false,
                tiers,
            };
            // Stable versions deduplicate unchanged rates; retrieval time remains separate.
            price.version = format!(
                "models-dev-{}",
                &hash(&format!("{}:{}:{}", id, cost, price.source))[..16]
            );
            prices.push(price);
        }
        ensure!(
            prices.len() > before,
            "Catalog has no complete {provider} token prices"
        );
    }
    Ok(prices)
}

impl Engine {
    pub fn pricing_refresh_status(&self) -> Result<Value> {
        Ok(self.get("config", "pricing_refresh")?.unwrap_or(json!({
            "last_attempt":null,"last_success":null,"error":null,"updated_models":0
        })))
    }

    pub fn pricing_refresh_due(&self, at: DateTime<Utc>) -> Result<bool> {
        if self.is_demo || self.settings()?["pricing_auto_refresh"] != true {
            return Ok(false);
        }
        let status = self.pricing_refresh_status()?;
        let elapsed = |key: &str, seconds| {
            status[key]
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .is_none_or(|time| at.signed_duration_since(time).num_seconds() >= seconds)
        };
        Ok(elapsed("last_success", 24 * 3600) && elapsed("last_attempt", 3600))
    }

    pub fn record_pricing_failure(&self, attempted_at: &str, error: &str) -> Result<()> {
        let mut status = self.pricing_refresh_status()?;
        status["last_attempt"] = json!(attempted_at);
        status["error"] = json!(error);
        self.put("config", "pricing_refresh", &status)
    }

    pub fn import_pricing_catalog(&self, catalog: &Value, retrieved_at: &str) -> Result<Value> {
        ensure!(
            !self.is_demo,
            "Live pricing refresh is unavailable in demo mode"
        );
        DateTime::parse_from_rfc3339(retrieved_at).context("Invalid catalog retrieval time")?;
        let prices = catalog_prices(catalog, retrieved_at)?;
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut updated = 0;
            for price in prices {
                if self
                    .get("prices", &price.model)?
                    .is_some_and(|p| p["override"] == true)
                {
                    continue;
                }
                let value = serde_json::to_value(&price)?;
                let previous = self.get("prices", &price.model)?;
                if previous
                    .as_ref()
                    .is_none_or(|p| p["version"] != value["version"])
                {
                    updated += 1;
                }
                self.put("prices", &price.model, &value)?;
                // Keep the first copy of a version immutable for historical exports.
                let key = format!("price:{}:{}", price.model, price.version);
                if self.get("config", &key)?.is_none() {
                    self.put("config", &key, &value)?;
                }
            }
            let status = json!({"last_attempt":retrieved_at,"last_success":retrieved_at,"error":null,"updated_models":updated});
            self.put("config", "pricing_refresh", &status)?;
            Ok(json!({"ok":true,"updated_models":updated}))
        })();
        self.db
            .execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    const AT: &str = "2026-10-07T12:00:00Z";
    fn catalog() -> Value {
        serde_json::from_str(include_str!("../../../fixtures/pricing-catalog.json")).unwrap()
    }
    fn engine() -> Engine {
        Engine::isolated(std::path::Path::new(":memory:")).unwrap()
    }
    fn event() -> Event {
        serde_json::from_value(json!({
            "id":"test", "session_id":"session", "timestamp":AT, "model":"gpt-6-sol",
            "usage":Usage::tokens(300_000, 1000, 1000, 0, 0), "scope":"session-own", "kind":"response-record",
            "request_id":null, "turn_id":null, "source_path":"test.jsonl", "source_line":1,
            "parser_version":PARSER, "pricing_version":null, "reported":true, "warnings":[], "duration_ms":null
        })).unwrap()
    }
    #[test]
    fn refresh_preserves_versions_and_uses_catalog_context_rates() {
        let e = engine();
        let mut old = event();
        e.apply_price(&mut old).unwrap();
        let previous = old.clone();
        let history_key = format!("price:gpt-6-sol:{}", old.pricing_version.as_ref().unwrap());
        let history = e.get("config", &history_key).unwrap();
        assert_eq!(
            e.import_pricing_catalog(&catalog(), AT).unwrap()["updated_models"],
            2
        );
        e.apply_price(&mut old).unwrap();
        assert_eq!(old.usage.nano, previous.usage.nano);
        assert_eq!(old.pricing_version, previous.pricing_version);
        assert_eq!(e.get("config", &history_key).unwrap(), history);
        let mut new = event();
        e.apply_price(&mut new).unwrap();
        assert_eq!(new.usage.nano, 2_121_700_000);
        assert!(new.pricing_version.unwrap().starts_with("models-dev-"));
        assert!(e.get("prices", "gpt-incomplete").unwrap().is_none());
        let new_history = e
            .get(
                "config",
                &format!(
                    "price:gpt-6-sol:{}",
                    e.get("prices", "gpt-6-sol").unwrap().unwrap()["version"]
                        .as_str()
                        .unwrap()
                ),
            )
            .unwrap();
        assert_eq!(
            e.import_pricing_catalog(&catalog(), "2026-10-08T12:00:00Z")
                .unwrap()["updated_models"],
            0
        );
        assert_eq!(
            e.get(
                "config",
                &format!(
                    "price:gpt-6-sol:{}",
                    e.get("prices", "gpt-6-sol").unwrap().unwrap()["version"]
                        .as_str()
                        .unwrap()
                )
            )
            .unwrap(),
            new_history
        );
    }
    #[test]
    fn new_models_use_published_context_tiers_despite_parser_warning() {
        let e = engine();
        let mut catalog = catalog();
        catalog["openai"]["models"]["gpt-future"] =
            catalog["openai"]["models"]["gpt-6-sol"].clone();
        e.import_pricing_catalog(&catalog, AT).unwrap();
        let mut event = event();
        event.model = "gpt-future".into();
        event.warnings.push(
            "Pricing unavailable: long-context tier requires a verified rate override".into(),
        );
        e.apply_price(&mut event).unwrap();
        assert_eq!(event.usage.unpriced_tokens, 0);
        assert_eq!(event.usage.nano, 2_121_700_000);
        assert!(!event
            .warnings
            .iter()
            .any(|w| w.starts_with("Pricing unavailable:")));
    }
    #[test]
    fn overrides_and_invalid_catalog_keep_saved_prices() {
        let e = engine();
        e.save_price(
            &json!({"model":"gpt-6-sol","input":99,"output":101,"cache_read":1,"cache_write":2}),
        )
        .unwrap();
        let saved = e.get("prices", "gpt-6-sol").unwrap();
        assert_eq!(
            e.import_pricing_catalog(&catalog(), AT).unwrap()["updated_models"],
            1
        );
        assert_eq!(e.get("prices", "gpt-6-sol").unwrap(), saved);
        let before = e.list("prices").unwrap();
        let status = e.pricing_refresh_status().unwrap();
        let mut invalid = catalog();
        invalid["anthropic"]["models"]["claude-sonnet-5-5"]["cost"]["input"] = json!(-1);
        assert!(e.import_pricing_catalog(&invalid, AT).is_err());
        assert!(e.import_pricing_catalog(&json!({}), AT).is_err());
        assert_eq!(e.list("prices").unwrap(), before);
        assert_eq!(e.pricing_refresh_status().unwrap(), status);
    }
    #[test]
    fn daily_schedule_retry_disable_and_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("prices.sqlite");
        let mut e = Engine::isolated(&path).unwrap();
        let at = DateTime::parse_from_rfc3339(AT)
            .unwrap()
            .with_timezone(&Utc);
        assert!(e.pricing_refresh_due(at).unwrap());
        e.record_pricing_failure(AT, "Offline").unwrap();
        assert!(!e.pricing_refresh_due(at + Duration::minutes(59)).unwrap());
        assert!(e.pricing_refresh_due(at + Duration::hours(1)).unwrap());
        e.import_pricing_catalog(&catalog(), AT).unwrap();
        assert!(e.pricing_refresh_status().unwrap()["error"].is_null());
        drop(e);
        e = Engine::isolated(&path).unwrap();
        assert!(!e.pricing_refresh_due(at + Duration::hours(23)).unwrap());
        assert!(e.pricing_refresh_due(at + Duration::days(1)).unwrap());
        e.dispatch(
            "settings_save",
            json!({"settings":{"pricing_auto_refresh":false}}),
        )
        .unwrap();
        assert!(!e.pricing_refresh_due(at + Duration::days(10)).unwrap());
        assert!(!Engine::demo().unwrap().pricing_refresh_due(at).unwrap());
    }
    #[test]
    fn old_settings_gain_refresh_default_without_losing_preferences() {
        let mut e = engine();
        let mut settings = settings_default();
        settings
            .as_object_mut()
            .unwrap()
            .remove("pricing_auto_refresh");
        settings["theme"] = json!("light");
        e.put("config", "settings", &settings).unwrap();
        assert_eq!(e.settings().unwrap()["pricing_auto_refresh"], true);
        assert_eq!(e.settings().unwrap()["theme"], "light");
        assert!(e
            .dispatch(
                "settings_save",
                json!({"settings":{"pricing_auto_refresh":"yes"}})
            )
            .is_err());
        e.dispatch(
            "settings_save",
            json!({"settings":{"pricing_auto_refresh":false}}),
        )
        .unwrap();
        assert_eq!(e.settings().unwrap()["pricing_auto_refresh"], false);
    }
}
