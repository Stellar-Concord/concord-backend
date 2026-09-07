use crate::auth::{challenge_message, issue_jwt, verify_wallet_signature, ChallengeStore};
use crate::error::AppError;
use crate::state::AppState;
use axum::extract::State;
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct ChallengeRequest {
    address: String,
}

#[derive(Serialize)]
pub struct ChallengeResponse {
    nonce: String,
    message: String,
}

pub async fn challenge(
    State(challenges): State<Arc<ChallengeStore>>,
    Json(req): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, AppError> {
    if stellar_strkey::ed25519::PublicKey::from_string(&req.address).is_err() {
        return Err(AppError::BadRequest("invalid Stellar address".into()));
    }
    let nonce = challenges.issue(&req.address);
    let message = String::from_utf8(challenge_message(&nonce)).unwrap();
    Ok(Json(ChallengeResponse { nonce, message }))
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    address: String,
    nonce: String,
    /// Base64-encoded ed25519 signature over `challenge_message(nonce)`.
    signature: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    token: String,
}

pub async fn verify(
    State(state): State<AppState>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    if !state
        .challenges
        .verify_and_consume(&req.address, &req.nonce)
    {
        return Err(AppError::Unauthorized(
            "challenge not found, already used, or expired".into(),
        ));
    }

    let signature = BASE64
        .decode(&req.signature)
        .map_err(|_| AppError::BadRequest("signature must be base64-encoded".into()))?;
    verify_wallet_signature(&req.address, &challenge_message(&req.nonce), &signature)?;

    let token = issue_jwt(&state.jwt_secret, &req.address)?;
    Ok(Json(TokenResponse { token }))
}
