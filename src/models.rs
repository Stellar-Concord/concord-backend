use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscrowStatus {
    Created,
    Funded,
    InProgress,
    Completed,
    Cancelled,
}

impl EscrowStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EscrowStatus::Created => "created",
            EscrowStatus::Funded => "funded",
            EscrowStatus::InProgress => "in_progress",
            EscrowStatus::Completed => "completed",
            EscrowStatus::Cancelled => "cancelled",
        }
    }
}

impl FromStr for EscrowStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "created" => Ok(EscrowStatus::Created),
            "funded" => Ok(EscrowStatus::Funded),
            "in_progress" => Ok(EscrowStatus::InProgress),
            "completed" => Ok(EscrowStatus::Completed),
            "cancelled" => Ok(EscrowStatus::Cancelled),
            other => anyhow::bail!("unknown escrow status: {other}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisputeStatus {
    Open,
    Resolved,
}

impl DisputeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DisputeStatus::Open => "open",
            DisputeStatus::Resolved => "resolved",
        }
    }
}

impl FromStr for DisputeStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(DisputeStatus::Open),
            "resolved" => Ok(DisputeStatus::Resolved),
            other => anyhow::bail!("unknown dispute status: {other}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EscrowRow {
    pub id: i64,
    pub contract_id: String,
    pub client: String,
    pub provider: String,
    pub arbitrator: String,
    pub token: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub updated_ledger: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MilestoneRow {
    pub escrow_id: i64,
    pub milestone_id: i32,
    pub description: String,
    pub amount: BigDecimal,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DisputeRow {
    pub escrow_id: i64,
    pub milestone_id: i32,
    pub raised_by: String,
    pub reason: String,
    pub status: String,
    pub resolution_kind: Option<String>,
    pub resolution_provider_bps: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EscrowDetail {
    #[serde(flatten)]
    pub escrow: EscrowRow,
    pub milestones: Vec<MilestoneRow>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct WebhookRow {
    pub id: uuid::Uuid,
    pub escrow_id: i64,
    pub owner_address: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
}
