use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub struct SorobanRpcClient {
    http: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpcEvent {
    #[serde(rename = "contractId")]
    pub contract_id: String,
    pub ledger: u32,
    #[serde(rename = "pagingToken")]
    pub paging_token: String,
    pub topic: Vec<String>,
    pub value: String,
    #[serde(rename = "inSuccessfulContractCall")]
    pub in_successful_contract_call: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetEventsResult {
    pub events: Vec<RpcEvent>,
    #[serde(rename = "latestLedger")]
    pub latest_ledger: u32,
    pub cursor: Option<String>,
}

#[derive(Serialize)]
struct JsonRpcRequest<P> {
    jsonrpc: &'static str,
    id: u32,
    method: &'static str,
    params: P,
}

#[derive(Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Serialize)]
struct EventFilter<'a> {
    #[serde(rename = "type")]
    filter_type: &'static str,
    #[serde(rename = "contractIds")]
    contract_ids: [&'a str; 1],
}

#[derive(Serialize)]
struct Pagination<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<&'a str>,
    limit: u32,
}

#[derive(Serialize)]
struct GetEventsParams<'a> {
    #[serde(rename = "startLedger", skip_serializing_if = "Option::is_none")]
    start_ledger: Option<u32>,
    filters: [EventFilter<'a>; 1],
    pagination: Pagination<'a>,
}

impl SorobanRpcClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    /// Fetches the next page of contract events for `contract_id`.
    ///
    /// Pass `cursor` to resume after a previous page (most calls after the
    /// first), or `start_ledger` on the very first call for this contract.
    pub async fn get_events(
        &self,
        contract_id: &str,
        start_ledger: Option<u32>,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<GetEventsResult> {
        let params = GetEventsParams {
            start_ledger: if cursor.is_some() { None } else { start_ledger },
            filters: [EventFilter {
                filter_type: "contract",
                contract_ids: [contract_id],
            }],
            pagination: Pagination { cursor, limit },
        };
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "getEvents",
            params,
        };

        let response: JsonRpcResponse<GetEventsResult> = self
            .http
            .post(&self.base_url)
            .json(&request)
            .send()
            .await
            .context("getEvents request failed")?
            .json()
            .await
            .context("failed to parse getEvents response")?;

        if let Some(err) = response.error {
            bail!("soroban rpc error {}: {}", err.code, err.message);
        }
        response.result.context("getEvents response had no result")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_get_events_request_with_cursor() {
        let params = GetEventsParams {
            start_ledger: None,
            filters: [EventFilter {
                filter_type: "contract",
                contract_ids: ["CABC"],
            }],
            pagination: Pagination {
                cursor: Some("123-0"),
                limit: 100,
            },
        };
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "getEvents",
            params,
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["method"], "getEvents");
        assert_eq!(json["params"]["pagination"]["cursor"], "123-0");
        assert!(json["params"].get("startLedger").is_none());
    }
}
