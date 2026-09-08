use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
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
    /// When the ledger this event is in was closed -- the closest thing to
    /// an authoritative "when did this actually happen" available from an
    /// event alone (contract events don't all carry their own timestamp
    /// field), and more accurate than the indexer's own processing time,
    /// which lags by up to the poll interval.
    #[serde(rename = "ledgerClosedAt")]
    pub ledger_closed_at: DateTime<Utc>,
    /// This event's own cursor position, usable as a paging token if the
    /// response as a whole doesn't carry one.
    pub id: String,
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

#[derive(Debug, Deserialize)]
pub struct GetLatestLedgerResult {
    pub sequence: u32,
}

impl SorobanRpcClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    async fn call<P: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<T> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };

        let response: JsonRpcResponse<T> = self
            .http
            .post(&self.base_url)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("{method} request failed"))?
            .json()
            .await
            .with_context(|| format!("failed to parse {method} response"))?;

        if let Some(err) = response.error {
            bail!("soroban rpc error {}: {}", err.code, err.message);
        }
        response
            .result
            .with_context(|| format!("{method} response had no result"))
    }

    /// Fetches the next page of contract events for `contract_id`.
    ///
    /// Pass `cursor` to resume after a previous page (most calls after the
    /// first), or `start_ledger` on the very first call for this contract.
    /// `start_ledger` must be a positive, currently-retained ledger sequence
    /// — the RPC rejects both `0` and ledgers outside its retention window.
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
        self.call("getEvents", params).await
    }

    pub async fn get_latest_ledger(&self) -> Result<u32> {
        let result: GetLatestLedgerResult =
            self.call("getLatestLedger", serde_json::json!({})).await?;
        Ok(result.sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `getEvents` response captured from soroban-testnet.stellar.org
    /// (fields not read by `RpcEvent`/`GetEventsResult` included verbatim,
    /// to guard against assuming fields — like the nonexistent
    /// `pagingToken` — that the live RPC never actually sends).
    const SAMPLE_RESPONSE: &str = r#"{
        "events": [
            {
                "type": "contract",
                "ledger": 4555815,
                "ledgerClosedAt": "2026-09-07T17:37:42Z",
                "contractId": "CCV6TXXCFDFC743XMQBRICV4HFMKRPDDCPUQFKOWVYZ3BNJEGEDQODSO",
                "id": "0019567076431654912-0000000000",
                "operationIndex": 0,
                "transactionIndex": 7,
                "txHash": "47904d90dab125163eae92caa593b49068069fee5c90cba1f5922257e252c334",
                "inSuccessfulContractCall": true,
                "topic": ["AAAADwAAAA5lc2Nyb3dfY3JlYXRlZAAA", "AAAABQAAAAAAAAAA"],
                "value": "AAAAEQ=="
            }
        ],
        "cursor": "0019567093611524096-0000000001",
        "latestLedger": 4555876,
        "oldestLedger": 4434917,
        "latestLedgerCloseTime": "1788802967",
        "oldestLedgerCloseTime": "1788198172"
    }"#;

    #[test]
    fn deserializes_real_get_events_response_shape() {
        let result: GetEventsResult = serde_json::from_str(SAMPLE_RESPONSE).unwrap();
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].id, "0019567076431654912-0000000000");
        assert_eq!(result.events[0].ledger, 4555815);
        assert_eq!(
            result.events[0].ledger_closed_at.to_rfc3339(),
            "2026-09-07T17:37:42+00:00"
        );
        assert!(result.events[0].in_successful_contract_call);
        assert_eq!(
            result.cursor.as_deref(),
            Some("0019567093611524096-0000000001")
        );
        assert_eq!(result.latest_ledger, 4555876);
    }

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
