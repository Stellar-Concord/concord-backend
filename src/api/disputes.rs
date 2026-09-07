use crate::db;
use crate::error::AppError;
use crate::models::{DisputeRow, DisputeStatus};
use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use sqlx::PgPool;
use std::str::FromStr;

#[derive(Deserialize)]
pub struct ListDisputesQuery {
    /// Defaults to "open" — pass `status=resolved` or `status=all` to widen.
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

pub async fn list_disputes(
    State(pool): State<PgPool>,
    Query(query): Query<ListDisputesQuery>,
) -> Result<Json<Vec<DisputeRow>>, AppError> {
    let status = match query.status.as_deref() {
        None => Some(DisputeStatus::Open.as_str()),
        Some("all") => None,
        Some(other) => Some(
            DisputeStatus::from_str(other)
                .map_err(|_| AppError::BadRequest(format!("invalid status: {other}")))?
                .as_str(),
        ),
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);
    let disputes = db::list_disputes(&pool, status, limit, offset).await?;
    Ok(Json(disputes))
}
