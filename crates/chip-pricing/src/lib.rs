//! Native pricing HTTP boundary. The accounting engine itself stays offline.
use anyhow::{ensure, Context, Result};
use chip_core::{LocalIndex, UserError, PRICING_CATALOG_URL};
use chrono::Utc;
use serde_json::Value;
use std::{
    io::Read,
    sync::{Arc, Mutex},
    time::Duration,
};

const BODY_LIMIT: u64 = 16 * 1024 * 1024;

fn fetch_catalog() -> Result<Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("ChipCount/0.1.1 pricing refresh")
        .build()?;
    let response = client.get(PRICING_CATALOG_URL).send()?.error_for_status()?;
    ensure!(
        response.status().is_success(),
        "Catalog request returned {}",
        response.status()
    );
    ensure!(
        response.content_length().is_none_or(|n| n <= BODY_LIMIT),
        "Pricing catalog exceeds 16 MB"
    );
    let mut body = vec![];
    response.take(BODY_LIMIT + 1).read_to_end(&mut body)?;
    ensure!(
        body.len() as u64 <= BODY_LIMIT,
        "Pricing catalog exceeds 16 MB"
    );
    serde_json::from_slice(&body).context("Pricing catalog is not valid JSON")
}

#[derive(Default)]
pub struct PricingUpdater {
    flight: Mutex<()>,
}
impl PricingUpdater {
    pub fn refresh(&self, index: &Mutex<LocalIndex>, force: bool) -> Result<Value> {
        self.refresh_with(index, force, fetch_catalog)
    }

    fn refresh_with(
        &self,
        index: &Mutex<LocalIndex>,
        force: bool,
        fetch: impl FnOnce() -> Result<Value>,
    ) -> Result<Value> {
        let _flight = self
            .flight
            .try_lock()
            .map_err(|_| anyhow::anyhow!("A pricing refresh is already running"))?;
        if !force
            && !index
                .lock()
                .map_err(|_| anyhow::anyhow!("Index mutex was poisoned"))?
                .engine()?
                .pricing_refresh_due(Utc::now())?
        {
            return Ok(serde_json::json!({"ok":true,"skipped":true}));
        }
        // Fetch without holding the accounting mutex: indexing and UI remain available.
        let attempted_at = Utc::now().to_rfc3339();
        let catalog = fetch();
        let mut index = index
            .lock()
            .map_err(|_| anyhow::anyhow!("Index mutex was poisoned"))?;
        let engine = index.engine()?;
        let result =
            catalog.and_then(|catalog| engine.import_pricing_catalog(&catalog, &attempted_at));
        if let Err(error) = &result {
            engine.record_pricing_failure(&attempted_at, &format!("{error:#}"))?;
        }
        result.map_err(|error| UserError::new("pricing_unavailable", "API prices could not be refreshed.", "Check your internet connection and retry in Settings → Pricing. Saved prices are retained; automatic refresh retries in an hour.", format!("{error:#}")).into())
    }

    pub fn start(
        self: Arc<Self>,
        index: Arc<Mutex<LocalIndex>>,
        stopping: Arc<std::sync::atomic::AtomicBool>,
        changed: impl Fn() + Send + 'static,
    ) -> std::io::Result<()> {
        std::thread::Builder::new()
            .name("chip-pricing-refresh".into())
            .spawn(move || {
                while !stopping.load(std::sync::atomic::Ordering::Relaxed) {
                    match self.refresh(&index, false) {
                        Ok(result) if result["skipped"] == true => {}
                        _ => changed(),
                    }
                    std::thread::sleep(Duration::from_secs(60));
                }
            })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_fetch_retries_later_without_holding_accounting_lock() {
        let dir = tempfile::tempdir().unwrap();
        let index = Mutex::new(LocalIndex::open(&dir.path().join("test.sqlite"), false));
        let updater = PricingUpdater::default();
        let before = index
            .lock()
            .unwrap()
            .dispatch("snapshot", serde_json::json!({}))
            .unwrap()["prices"]
            .clone();
        let result = updater.refresh_with(&index, true, || {
            assert!(
                index.try_lock().is_ok(),
                "network fetch must not block indexing"
            );
            anyhow::bail!("offline")
        });
        assert!(result.is_err());
        let after = index
            .lock()
            .unwrap()
            .dispatch("snapshot", serde_json::json!({}))
            .unwrap();
        assert_eq!(before, after["prices"]);
        assert_eq!(after["pricing_refresh"]["error"], "offline");
        let skipped = updater
            .refresh_with(&index, false, || panic!("must back off"))
            .unwrap();
        assert_eq!(skipped["skipped"], true);
        let success = updater
            .refresh_with(&index, true, || {
                Ok(serde_json::from_str(include_str!(
                    "../../../fixtures/pricing-catalog.json"
                ))?)
            })
            .unwrap();
        assert_eq!(success["updated_models"], 2);
        assert!(index
            .lock()
            .unwrap()
            .engine()
            .unwrap()
            .pricing_refresh_status()
            .unwrap()["error"]
            .is_null());
    }
}
