use crate::db;
use crate::error::AppError;
use crate::models::{EscrowDetail, EscrowStatus};
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::PgPool;
use std::str::FromStr;

#[derive(Deserialize)]
pub struct ListEscrowsQuery {
    client: Option<String>,
    provider: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

pub async fn get_escrow(
    State(pool): State<PgPool>,
    Path(id): Path<i64>,
) -> Result<Json<EscrowDetail>, AppError> {
    let escrow = db::get_escrow(&pool, id).await?.ok_or(AppError::NotFound)?;
    let milestones = db::list_milestones(&pool, id).await?;
    Ok(Json(EscrowDetail { escrow, milestones }))
}

pub async fn list_escrows(
    State(pool): State<PgPool>,
    Query(query): Query<ListEscrowsQuery>,
) -> Result<Json<Vec<crate::models::EscrowRow>>, AppError> {
    let status = query
        .status
        .map(|s| {
            EscrowStatus::from_str(&s)
                .map(|s| s.as_str().to_string())
                .map_err(|_| AppError::BadRequest(format!("invalid status: {s}")))
        })
        .transpose()?;

    let filter = db::EscrowFilter {
        client: query.client,
        provider: query.provider,
        status,
        limit: query.limit.unwrap_or(50).clamp(1, 200),
        offset: query.offset.unwrap_or(0).max(0),
    };
    let escrows = db::list_escrows(&pool, &filter).await?;
    Ok(Json(escrows))
}
