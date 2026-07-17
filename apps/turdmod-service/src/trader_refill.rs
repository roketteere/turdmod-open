// Hourly trader-funds top-up — keeps every trader's available_funds at REFILL_VALUE
// so traders can always buy from players. Interval-based by design (NO continuous
// polling, per Joel 2026-06-17): one write-burst per hour, nothing in between.
// @dep bridge `traderFunds` handler (sets the in-memory value SCUM then persists).
// @ctx The funds field is NOT reflected, so the offset is resolved via traderFunds
//   "scan" (the int32 uniform + funds-plausible across all 32 trader components).
//   Pin FORCED_OFFSET once verified vs economy_traders.available_funds to skip scan.

use std::time::Duration;

use crate::events::GameEvent;
use crate::pipe_rpc;
use crate::registry::{Mod, ModCtx, Outcome};

const REFILL_VALUE: i64 = 99_999;
const REFILL_INTERVAL: Duration = Duration::from_secs(3600); // hourly
// 0 = auto-detect the funds offset each tick via `traderFunds scan` (writes only when
// the scan is unambiguous — exactly one candidate). Set to the verified offset (from a
// one-time scan checked against the DB) to pin it and skip the scan.
const FORCED_OFFSET: i64 = 0;

pub struct TraderRefill;
impl TraderRefill {
    pub fn new() -> Self { Self }
}

#[async_trait::async_trait]
impl Mod for TraderRefill {
    fn name(&self) -> &'static str { "trader_refill" }
    fn interval(&self) -> Option<Duration> { Some(REFILL_INTERVAL) }

    async fn tick(&self, _ctx: &ModCtx) {
        // Resolve the in-memory funds offset: pinned, or auto-detected via scan.
        let offset: i64 = if FORCED_OFFSET != 0 {
            FORCED_OFFSET
        } else {
            match pipe_rpc::call("traderFunds", Some(serde_json::json!({ "mode": "scan" }))).await {
                Ok(v) => {
                    let cands = v.get("candidates").and_then(|c| c.as_array()).cloned().unwrap_or_default();
                    // Only auto-write when there's exactly ONE candidate — never risk
                    // writing a wrong offset across all 32 trader components (crash).
                    if cands.len() == 1 {
                        cands[0].get("offset").and_then(|o| o.as_i64()).unwrap_or(0)
                    } else {
                        tracing::warn!(
                            "trader_refill: scan returned {} fund-offset candidates (need exactly 1) \
                             — skipping; pin FORCED_OFFSET once verified",
                            cands.len()
                        );
                        return;
                    }
                }
                Err(e) => {
                    tracing::warn!("trader_refill: traderFunds scan failed: {}", e);
                    return;
                }
            }
        };
        if offset == 0 {
            tracing::warn!("trader_refill: no funds offset resolved — skipping top-up");
            return;
        }

        match pipe_rpc::call(
            "traderFunds",
            Some(serde_json::json!({
                "mode": "set",
                "offset": offset.to_string(),
                "value": REFILL_VALUE.to_string(),
            })),
        )
        .await
        {
            Ok(v) => {
                let n = v.get("traders").and_then(|x| x.as_u64()).unwrap_or(0);
                tracing::info!("trader_refill: topped {} traders to {} (offset {})", n, REFILL_VALUE, offset);
            }
            Err(e) => tracing::warn!("trader_refill: traderFunds set failed: {}", e),
        }
    }

    async fn handle(&self, _ev: &GameEvent, _ctx: &ModCtx) -> Outcome {
        Outcome::Ignored
    }
}
