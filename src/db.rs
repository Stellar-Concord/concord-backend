use crate::models::{DisputeRow, EscrowRow, MilestoneRow, WebhookRow};
use bigdecimal::BigDecimal;
use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn init_pool(database_url: &str) -> sqlx::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub struct EscrowFilter {
    pub client: Option<String>,
    pub provider: Option<String>,
    pub status: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

pub async fn get_escrow(pool: &PgPool, id: i64) -> sqlx::Result<Option<EscrowRow>> {
    sqlx::query_as::<_, EscrowRow>("SELECT * FROM escrows WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn list_milestones(pool: &PgPool, escrow_id: i64) -> sqlx::Result<Vec<MilestoneRow>> {
    sqlx::query_as::<_, MilestoneRow>(
        "SELECT * FROM milestones WHERE escrow_id = $1 ORDER BY milestone_id",
    )
    .bind(escrow_id)
    .fetch_all(pool)
    .await
}

pub async fn list_escrows(pool: &PgPool, filter: &EscrowFilter) -> sqlx::Result<Vec<EscrowRow>> {
    sqlx::query_as::<_, EscrowRow>(
        r#"
        SELECT * FROM escrows
        WHERE ($1::text IS NULL OR client = $1)
          AND ($2::text IS NULL OR provider = $2)
          AND ($3::text IS NULL OR status = $3)
        ORDER BY id DESC
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(&filter.client)
    .bind(&filter.provider)
    .bind(&filter.status)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await
}

pub async fn list_disputes(
    pool: &PgPool,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> sqlx::Result<Vec<DisputeRow>> {
    sqlx::query_as::<_, DisputeRow>(
        r#"
        SELECT * FROM disputes
        WHERE ($1::text IS NULL OR status = $1)
        ORDER BY created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_escrow(
    pool: &PgPool,
    id: i64,
    contract_id: &str,
    client: &str,
    provider: &str,
    arbitrator: &str,
    token: &str,
    status: &str,
    ledger: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO escrows (id, contract_id, client, provider, arbitrator, token, status, updated_ledger)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (id) DO UPDATE SET
            status = EXCLUDED.status,
            updated_at = now(),
            updated_ledger = EXCLUDED.updated_ledger
        WHERE EXCLUDED.updated_ledger >= escrows.updated_ledger
        "#,
    )
    .bind(id)
    .bind(contract_id)
    .bind(client)
    .bind(provider)
    .bind(arbitrator)
    .bind(token)
    .bind(status)
    .bind(ledger)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_escrow_status(
    pool: &PgPool,
    id: i64,
    status: &str,
    ledger: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE escrows SET status = $2, updated_at = now(), updated_ledger = $3
        WHERE id = $1 AND $3 >= updated_ledger
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(ledger)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_milestone(
    pool: &PgPool,
    escrow_id: i64,
    milestone_id: i32,
    description: &str,
    amount: BigDecimal,
    status: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO milestones (escrow_id, milestone_id, description, amount, status)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (escrow_id, milestone_id) DO UPDATE SET
            status = EXCLUDED.status,
            updated_at = now()
        "#,
    )
    .bind(escrow_id)
    .bind(milestone_id)
    .bind(description)
    .bind(amount)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_milestone_status(
    pool: &PgPool,
    escrow_id: i64,
    milestone_id: i32,
    status: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE milestones SET status = $3, updated_at = now() WHERE escrow_id = $1 AND milestone_id = $2",
    )
    .bind(escrow_id)
    .bind(milestone_id)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_dispute(
    pool: &PgPool,
    escrow_id: i64,
    milestone_id: i32,
    raised_by: &str,
    reason: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        INSERT INTO disputes (escrow_id, milestone_id, raised_by, reason, status)
        VALUES ($1, $2, $3, $4, 'open')
        ON CONFLICT (escrow_id, milestone_id) DO NOTHING
        "#,
    )
    .bind(escrow_id)
    .bind(milestone_id)
    .bind(raised_by)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn resolve_dispute(
    pool: &PgPool,
    escrow_id: i64,
    milestone_id: i32,
    resolution_kind: &str,
    resolution_provider_bps: Option<i32>,
) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        UPDATE disputes SET
            status = 'resolved',
            resolution_kind = $3,
            resolution_provider_bps = $4,
            resolved_at = now()
        WHERE escrow_id = $1 AND milestone_id = $2
        "#,
    )
    .bind(escrow_id)
    .bind(milestone_id)
    .bind(resolution_kind)
    .bind(resolution_provider_bps)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_webhook(
    pool: &PgPool,
    escrow_id: i64,
    owner_address: &str,
    url: &str,
) -> sqlx::Result<WebhookRow> {
    let id = uuid::Uuid::new_v4();
    sqlx::query_as::<_, WebhookRow>(
        r#"
        INSERT INTO webhooks (id, escrow_id, owner_address, url, created_at)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(escrow_id)
    .bind(owner_address)
    .bind(url)
    .bind(Utc::now())
    .fetch_one(pool)
    .await
}

pub async fn list_webhooks_for_escrow(
    pool: &PgPool,
    escrow_id: i64,
) -> sqlx::Result<Vec<WebhookRow>> {
    sqlx::query_as::<_, WebhookRow>("SELECT * FROM webhooks WHERE escrow_id = $1")
        .bind(escrow_id)
        .fetch_all(pool)
        .await
}

pub async fn delete_webhook(
    pool: &PgPool,
    id: uuid::Uuid,
    owner_address: &str,
) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM webhooks WHERE id = $1 AND owner_address = $2")
        .bind(id)
        .bind(owner_address)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_cursor(pool: &PgPool) -> sqlx::Result<(i64, Option<String>)> {
    let row: (i64, Option<String>) =
        sqlx::query_as("SELECT last_ledger, last_paging_token FROM indexer_cursor WHERE id = 1")
            .fetch_one(pool)
            .await?;
    Ok(row)
}

pub async fn set_cursor(pool: &PgPool, last_ledger: i64, paging_token: &str) -> sqlx::Result<()> {
    sqlx::query("UPDATE indexer_cursor SET last_ledger = $1, last_paging_token = $2 WHERE id = 1")
        .bind(last_ledger)
        .bind(paging_token)
        .execute(pool)
        .await?;
    Ok(())
}
