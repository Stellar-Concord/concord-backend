mod decode;
mod rpc;

use crate::config::Config;
use crate::db;
use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use rpc::{RpcEvent, SorobanRpcClient};
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use std::time::Duration;
use stellar_xdr::ScVal;

const PAGE_LIMIT: u32 = 100;
/// How far back to look on the very first poll (no cursor yet). RPC
/// providers only retain events for a limited window; this stays
/// comfortably inside typical retention (~1 day at Stellar's ~5s ledger
/// close time) while still covering a freshly deployed contract's history.
const BOOTSTRAP_LOOKBACK_LEDGERS: u32 = 17_280;

pub async fn run(pool: PgPool, config: Config) {
    let rpc = SorobanRpcClient::new(config.soroban_rpc_url.clone());
    let http = reqwest::Client::new();

    loop {
        match poll_once(&pool, &rpc, &http, &config).await {
            Ok(processed) if processed > 0 => {
                tracing::info!(processed, "indexer processed events");
            }
            Ok(_) => {}
            Err(err) => {
                tracing::error!(?err, "indexer poll failed, will retry");
            }
        }
        tokio::time::sleep(Duration::from_secs(config.indexer_poll_interval_secs)).await;
    }
}

async fn poll_once(
    pool: &PgPool,
    rpc: &SorobanRpcClient,
    http: &reqwest::Client,
    config: &Config,
) -> Result<usize> {
    let (last_ledger, last_cursor) = db::get_cursor(pool).await?;
    let start_ledger = if last_cursor.is_none() && last_ledger == 0 {
        let latest = rpc.get_latest_ledger().await?;
        Some(latest.saturating_sub(BOOTSTRAP_LOOKBACK_LEDGERS).max(1))
    } else {
        None
    };

    let result = rpc
        .get_events(
            &config.escrow_contract_id,
            start_ledger,
            last_cursor.as_deref(),
            PAGE_LIMIT,
        )
        .await
        .context("fetching events from soroban rpc")?;

    let mut processed = 0;
    for event in &result.events {
        if !event.in_successful_contract_call {
            continue;
        }
        if let Err(err) = process_event(pool, http, event).await {
            tracing::error!(?err, paging_token = %event.id, "failed to process event, skipping");
        }
        processed += 1;
    }

    // Always advance the cursor using the RPC's own pagination cursor, even
    // when this page had zero events, so we don't keep re-scanning an empty
    // ledger range on every poll.
    let next_cursor = result
        .cursor
        .or_else(|| result.events.last().map(|e| e.id.clone()));
    if let Some(cursor) = next_cursor {
        db::set_cursor(pool, result.latest_ledger as i64, &cursor).await?;
    }

    Ok(processed)
}

async fn process_event(pool: &PgPool, http: &reqwest::Client, event: &RpcEvent) -> Result<()> {
    let topics: Vec<ScVal> = event
        .topic
        .iter()
        .map(|t| decode::decode_scval_base64(t))
        .collect::<Result<_>>()?;
    let data = decode::decode_scval_base64(&event.value)?;

    let name = decode::sc_val_to_string(
        topics
            .first()
            .ok_or_else(|| anyhow::anyhow!("event has no topics"))?,
    )?;
    let escrow_id = decode::sc_val_to_u64(
        topics
            .get(1)
            .ok_or_else(|| anyhow::anyhow!("event missing escrow_id topic"))?,
    )? as i64;
    let ledger = event.ledger as i64;

    match name.as_str() {
        "escrow_created" => {
            let client = decode::sc_val_to_address(decode::map_get(&data, "client")?)?;
            let provider = decode::sc_val_to_address(decode::map_get(&data, "provider")?)?;
            let arbitrator = decode::sc_val_to_address(decode::map_get(&data, "arbitrator")?)?;
            let token = decode::sc_val_to_address(decode::map_get(&data, "token")?)?;
            db::upsert_escrow(
                pool,
                escrow_id,
                &event.contract_id,
                &client,
                &provider,
                &arbitrator,
                &token,
                "created",
                ledger,
            )
            .await?;

            let milestones = decode::decode_milestones(decode::map_get(&data, "milestones")?)?;
            for milestone in milestones {
                db::upsert_milestone(
                    pool,
                    escrow_id,
                    milestone.id as i32,
                    &milestone.description,
                    bigdecimal::BigDecimal::from(milestone.amount),
                    "pending",
                )
                .await?;
            }
        }
        "escrow_funded" => {
            db::set_escrow_status(pool, escrow_id, "funded", ledger).await?;
        }
        "milestone_submitted" => {
            let milestone_id =
                decode::sc_val_to_u32(decode::map_get(&data, "milestone_id")?)? as i32;
            db::set_escrow_status(pool, escrow_id, "in_progress", ledger).await?;
            db::set_milestone_status(pool, escrow_id, milestone_id, "submitted").await?;
        }
        "milestone_approved" => {
            let milestone_id =
                decode::sc_val_to_u32(decode::map_get(&data, "milestone_id")?)? as i32;
            db::set_milestone_status(pool, escrow_id, milestone_id, "released").await?;
        }
        "escrow_cancelled" => {
            db::set_escrow_status(pool, escrow_id, "cancelled", ledger).await?;
        }
        "escrow_completed" => {
            db::set_escrow_status(pool, escrow_id, "completed", ledger).await?;
        }
        "dispute_raised" => {
            let milestone_id =
                decode::sc_val_to_u32(decode::map_get(&data, "milestone_id")?)? as i32;
            let raised_by = decode::sc_val_to_address(decode::map_get(&data, "raised_by")?)?;
            let reason = decode::sc_val_to_string(decode::map_get(&data, "reason")?)?;
            db::set_milestone_status(pool, escrow_id, milestone_id, "disputed").await?;
            db::insert_dispute(pool, escrow_id, milestone_id, &raised_by, &reason).await?;
        }
        "dispute_resolved" => {
            let milestone_id =
                decode::sc_val_to_u32(decode::map_get(&data, "milestone_id")?)? as i32;
            let (kind, bps) = decode::decode_resolution(decode::map_get(&data, "resolution")?)?;
            db::set_milestone_status(pool, escrow_id, milestone_id, "resolved").await?;
            db::resolve_dispute(pool, escrow_id, milestone_id, kind, bps).await?;
        }
        other => {
            tracing::debug!(event = other, "ignoring unrecognized event");
            return Ok(());
        }
    }

    notify_webhooks(pool, http, escrow_id, &name).await;
    Ok(())
}

async fn notify_webhooks(pool: &PgPool, http: &reqwest::Client, escrow_id: i64, event: &str) {
    let webhooks = match db::list_webhooks_for_escrow(pool, escrow_id).await {
        Ok(w) => w,
        Err(err) => {
            tracing::error!(?err, "failed to load webhooks for escrow");
            return;
        }
    };
    let payload = json!({ "escrow_id": escrow_id, "event": event });
    // Serialize once and sign/send those exact bytes: reqwest's `.json()`
    // convenience would re-serialize, and while serde_json is deterministic
    // here, signing the bytes we actually transmit removes any doubt.
    let body =
        serde_json::to_vec(&payload).expect("json serialization of a simple map cannot fail");
    let timestamp = chrono::Utc::now().timestamp();

    for webhook in webhooks {
        let http = http.clone();
        let body = body.clone();
        let url = webhook.url.clone();
        let signature = sign_webhook_payload(&webhook.secret, timestamp, &body);
        tokio::spawn(async move {
            let result = http
                .post(&url)
                .header("content-type", "application/json")
                .header("x-concord-timestamp", timestamp.to_string())
                .header("x-concord-signature", format!("sha256={signature}"))
                .body(body)
                .timeout(Duration::from_secs(10))
                .send()
                .await;
            if let Err(err) = result {
                tracing::warn!(?err, url, "webhook delivery failed");
            }
        });
    }
}

/// HMAC-SHA256 over `"{timestamp}.{body}"`, hex-encoded. Consumers verify by
/// recomputing this with their own copy of the webhook's secret (shown once,
/// at registration) and comparing against the `x-concord-signature` header
/// -- proving the payload actually came from us and wasn't tampered with in
/// transit. Including the timestamp in the signed content, and expecting
/// consumers to reject old ones, guards against replay.
fn sign_webhook_payload(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_is_deterministic_and_key_dependent() {
        let body = br#"{"escrow_id":1,"event":"escrow_funded"}"#;
        let sig_a = sign_webhook_payload("secret-a", 1_700_000_000, body);
        let sig_a_again = sign_webhook_payload("secret-a", 1_700_000_000, body);
        let sig_b = sign_webhook_payload("secret-b", 1_700_000_000, body);
        let sig_different_time = sign_webhook_payload("secret-a", 1_700_000_001, body);

        assert_eq!(sig_a, sig_a_again);
        assert_ne!(sig_a, sig_b);
        assert_ne!(sig_a, sig_different_time);
        assert_eq!(sig_a.len(), 64); // hex-encoded SHA-256
    }
}
