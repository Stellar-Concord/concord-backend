use crate::auth::AuthUser;
use crate::db;
use crate::error::AppError;
use crate::models::{WebhookCreated, WebhookRow};
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct RegisterWebhookRequest {
    url: String,
}

/// Registers a webhook that fires whenever `escrow_id` changes state.
/// Only the escrow's client, provider, or arbitrator may register one.
pub async fn register_webhook(
    State(pool): State<PgPool>,
    AuthUser(address): AuthUser,
    Path(escrow_id): Path<i64>,
    Json(req): Json<RegisterWebhookRequest>,
) -> Result<Json<WebhookCreated>, AppError> {
    if !req.url.starts_with("https://") && !req.url.starts_with("http://") {
        return Err(AppError::BadRequest("url must be http(s)".into()));
    }

    let escrow = db::get_escrow(&pool, escrow_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if address != escrow.client && address != escrow.provider && address != escrow.arbitrator {
        return Err(AppError::Forbidden(
            "only the escrow's client, provider, or arbitrator may register a webhook".into(),
        ));
    }

    let webhook = db::insert_webhook(&pool, escrow_id, &address, &req.url).await?;
    Ok(Json(webhook.into()))
}

pub async fn list_webhooks(
    State(pool): State<PgPool>,
    AuthUser(address): AuthUser,
    Path(escrow_id): Path<i64>,
) -> Result<Json<Vec<WebhookRow>>, AppError> {
    let webhooks = db::list_webhooks_for_escrow(&pool, escrow_id)
        .await?
        .into_iter()
        .filter(|w| w.owner_address == address)
        .collect();
    Ok(Json(webhooks))
}

pub async fn delete_webhook(
    State(pool): State<PgPool>,
    AuthUser(address): AuthUser,
    Path((_escrow_id, webhook_id)): Path<(i64, uuid::Uuid)>,
) -> Result<(), AppError> {
    let deleted = db::delete_webhook(&pool, webhook_id, &address).await?;
    if !deleted {
        return Err(AppError::NotFound);
    }
    Ok(())
}
