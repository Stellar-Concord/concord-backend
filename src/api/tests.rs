//! Integration tests for the HTTP layer: real requests through the actual
//! router, against a real (ephemeral, per-test) Postgres database. Unlike
//! `indexer`/`auth`'s unit tests, these exercise the axum wiring itself --
//! extractors, JSON (de)serialization, status codes -- not just the
//! underlying logic.

use crate::auth::{issue_jwt, ChallengeStore};
use crate::db;
use crate::state::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use bigdecimal::BigDecimal;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;

const JWT_SECRET: &str = "test-secret";

fn app(pool: PgPool) -> Router {
    let state = AppState {
        pool,
        jwt_secret: JWT_SECRET.to_string(),
        challenges: Arc::new(ChallengeStore::new()),
    };
    super::router().with_state(state)
}

async fn request(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let response = tower::ServiceExt::oneshot(app.clone(), req).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

fn json_request(method: &str, uri: &str, body: Value, bearer: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn seed_escrow(
    pool: &PgPool,
    id: i64,
    client: &str,
    provider: &str,
    arbitrator: &str,
    status: &str,
) {
    db::upsert_escrow(
        pool,
        id,
        "CCONTRACT",
        client,
        provider,
        arbitrator,
        "CTOKEN",
        status,
        1,
    )
    .await
    .unwrap();
}

#[sqlx::test]
async fn get_escrow_returns_404_when_missing(pool: PgPool) {
    let app = app(pool);
    let (status, body) = request(&app, get("/escrows/999")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not found");
}

#[sqlx::test]
async fn get_escrow_returns_escrow_with_milestones(pool: PgPool) {
    seed_escrow(&pool, 1, "GCLIENT", "GPROVIDER", "GARBITRATOR", "created").await;
    db::upsert_milestone(&pool, 1, 0, "Design", BigDecimal::from(100), "pending")
        .await
        .unwrap();

    let app = app(pool);
    let (status, body) = request(&app, get("/escrows/1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], 1);
    assert_eq!(body["client"], "GCLIENT");
    assert_eq!(body["status"], "created");
    assert_eq!(body["milestones"][0]["description"], "Design");
}

#[sqlx::test]
async fn list_escrows_filters_by_client(pool: PgPool) {
    seed_escrow(&pool, 1, "GCLIENT_A", "GPROVIDER", "GARBITRATOR", "created").await;
    seed_escrow(&pool, 2, "GCLIENT_B", "GPROVIDER", "GARBITRATOR", "created").await;

    let app = app(pool);
    let (status, body) = request(&app, get("/escrows?client=GCLIENT_A")).await;
    assert_eq!(status, StatusCode::OK);
    let escrows = body.as_array().unwrap();
    assert_eq!(escrows.len(), 1);
    assert_eq!(escrows[0]["id"], 1);
}

#[sqlx::test]
async fn list_escrows_rejects_invalid_status(pool: PgPool) {
    let app = app(pool);
    let (status, body) = request(&app, get("/escrows?status=not_a_real_status")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("invalid status"));
}

#[sqlx::test]
async fn list_disputes_defaults_to_open(pool: PgPool) {
    seed_escrow(
        &pool,
        1,
        "GCLIENT",
        "GPROVIDER",
        "GARBITRATOR",
        "in_progress",
    )
    .await;
    db::insert_dispute(&pool, 1, 0, "GCLIENT", "not delivered")
        .await
        .unwrap();
    db::resolve_dispute(&pool, 1, 0, "release", None)
        .await
        .unwrap();
    seed_escrow(
        &pool,
        2,
        "GCLIENT2",
        "GPROVIDER2",
        "GARBITRATOR2",
        "in_progress",
    )
    .await;
    db::insert_dispute(&pool, 2, 0, "GCLIENT2", "still open")
        .await
        .unwrap();

    let app = app(pool);
    let (status, body) = request(&app, get("/disputes")).await;
    assert_eq!(status, StatusCode::OK);
    let disputes = body.as_array().unwrap();
    assert_eq!(disputes.len(), 1);
    assert_eq!(disputes[0]["escrow_id"], 2);
    assert_eq!(disputes[0]["status"], "open");
}

#[sqlx::test]
async fn list_disputes_all_widens_to_everything(pool: PgPool) {
    seed_escrow(
        &pool,
        1,
        "GCLIENT",
        "GPROVIDER",
        "GARBITRATOR",
        "in_progress",
    )
    .await;
    db::insert_dispute(&pool, 1, 0, "GCLIENT", "not delivered")
        .await
        .unwrap();
    db::resolve_dispute(&pool, 1, 0, "release", None)
        .await
        .unwrap();

    let app = app(pool);
    let (status, body) = request(&app, get("/disputes?status=all")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
}

#[sqlx::test]
async fn register_webhook_requires_auth(pool: PgPool) {
    seed_escrow(&pool, 1, "GCLIENT", "GPROVIDER", "GARBITRATOR", "created").await;
    let app = app(pool);
    let req = json_request(
        "POST",
        "/escrows/1/webhooks",
        serde_json::json!({ "url": "https://example.com/hook" }),
        None,
    );
    let (status, _) = request(&app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn register_webhook_rejects_non_party(pool: PgPool) {
    seed_escrow(&pool, 1, "GCLIENT", "GPROVIDER", "GARBITRATOR", "created").await;
    let app = app(pool);
    let token = issue_jwt(JWT_SECRET, "GSTRANGER").unwrap();
    let req = json_request(
        "POST",
        "/escrows/1/webhooks",
        serde_json::json!({ "url": "https://example.com/hook" }),
        Some(&token),
    );
    let (status, _) = request(&app, req).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn register_webhook_rejects_non_http_url(pool: PgPool) {
    seed_escrow(&pool, 1, "GCLIENT", "GPROVIDER", "GARBITRATOR", "created").await;
    let app = app(pool);
    let token = issue_jwt(JWT_SECRET, "GCLIENT").unwrap();
    let req = json_request(
        "POST",
        "/escrows/1/webhooks",
        serde_json::json!({ "url": "javascript:alert(1)" }),
        Some(&token),
    );
    let (status, _) = request(&app, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn webhook_lifecycle_register_list_delete(pool: PgPool) {
    seed_escrow(&pool, 1, "GCLIENT", "GPROVIDER", "GARBITRATOR", "created").await;
    let app = app(pool);
    let client_token = issue_jwt(JWT_SECRET, "GCLIENT").unwrap();

    let register_req = json_request(
        "POST",
        "/escrows/1/webhooks",
        serde_json::json!({ "url": "https://example.com/hook" }),
        Some(&client_token),
    );
    let (status, body) = request(&app, register_req).await;
    assert_eq!(status, StatusCode::OK);
    let webhook_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["owner_address"], "GCLIENT");

    // The provider (also a legitimate party) shouldn't see the client's hook.
    let provider_token = issue_jwt(JWT_SECRET, "GPROVIDER").unwrap();
    let list_req = Request::builder()
        .uri("/escrows/1/webhooks")
        .header("authorization", format!("Bearer {provider_token}"))
        .body(Body::empty())
        .unwrap();
    let (status, body) = request(&app, list_req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0);

    // The client sees their own.
    let list_req = Request::builder()
        .uri("/escrows/1/webhooks")
        .header("authorization", format!("Bearer {client_token}"))
        .body(Body::empty())
        .unwrap();
    let (status, body) = request(&app, list_req).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);

    // The provider can't delete the client's webhook.
    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/escrows/1/webhooks/{webhook_id}"))
        .header("authorization", format!("Bearer {provider_token}"))
        .body(Body::empty())
        .unwrap();
    let (status, _) = request(&app, delete_req).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The client can.
    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/escrows/1/webhooks/{webhook_id}"))
        .header("authorization", format!("Bearer {client_token}"))
        .body(Body::empty())
        .unwrap();
    let (status, _) = request(&app, delete_req).await;
    assert_eq!(status, StatusCode::OK);
}

#[sqlx::test]
async fn auth_challenge_rejects_invalid_address(pool: PgPool) {
    let app = app(pool);
    let req = json_request(
        "POST",
        "/auth/challenge",
        serde_json::json!({ "address": "not-a-real-address" }),
        None,
    );
    let (status, _) = request(&app, req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[sqlx::test]
async fn auth_verify_rejects_unknown_challenge(pool: PgPool) {
    let app = app(pool);
    let req = json_request(
        "POST",
        "/auth/verify",
        serde_json::json!({
            "address": "GCLIENT",
            "nonce": "never-issued",
            "signature": "AAAA",
        }),
        None,
    );
    let (status, _) = request(&app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
