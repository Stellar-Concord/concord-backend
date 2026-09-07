use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::RequestPartsExt;
use axum_extra::headers::authorization::Bearer;
use axum_extra::headers::Authorization;
use axum_extra::TypedHeader;
use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::AppError;

const CHALLENGE_TTL_SECS: i64 = 300;
const JWT_TTL_SECS: i64 = 3600;

/// Stellar wallet challenge/response authentication.
///
/// This is a lightweight, non-SEP-10 alternative: the client requests a
/// random nonce for their address, signs it with their Stellar keypair, and
/// exchanges the signature for a short-lived JWT. It proves control of the
/// address's private key without ever handling a password.
pub struct ChallengeStore {
    challenges: Mutex<HashMap<String, (String, DateTime<Utc>)>>,
}

impl Default for ChallengeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ChallengeStore {
    pub fn new() -> Self {
        Self {
            challenges: Mutex::new(HashMap::new()),
        }
    }

    pub fn issue(&self, address: &str) -> String {
        let mut nonce_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = hex::encode(nonce_bytes);

        let expires_at = Utc::now() + Duration::seconds(CHALLENGE_TTL_SECS);
        self.challenges
            .lock()
            .unwrap()
            .insert(address.to_string(), (nonce.clone(), expires_at));
        nonce
    }

    /// Verifies that `nonce` matches the last issued (unexpired) challenge
    /// for `address`, consuming it so it cannot be replayed.
    pub fn verify_and_consume(&self, address: &str, nonce: &str) -> bool {
        let mut challenges = self.challenges.lock().unwrap();
        match challenges.remove(address) {
            Some((expected_nonce, expires_at)) if expires_at > Utc::now() => {
                expected_nonce == nonce
            }
            _ => false,
        }
    }
}

/// The message a wallet actually signs is the nonce prefixed with a fixed
/// domain tag, to avoid a nonce being replayable as a signature over
/// unrelated data (e.g. a transaction).
pub fn challenge_message(nonce: &str) -> Vec<u8> {
    format!("Concord auth challenge: {nonce}").into_bytes()
}

pub fn verify_wallet_signature(
    address: &str,
    message: &[u8],
    signature: &[u8],
) -> Result<(), AppError> {
    let public_key = stellar_strkey::ed25519::PublicKey::from_string(address)
        .map_err(|_| AppError::BadRequest("invalid Stellar address".into()))?;
    let verifying_key = VerifyingKey::from_bytes(&public_key.0)
        .map_err(|_| AppError::BadRequest("invalid public key".into()))?;
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| AppError::BadRequest("signature must be 64 bytes".into()))?;
    let signature = Signature::from_bytes(&signature);
    verifying_key
        .verify(message, &signature)
        .map_err(|_| AppError::Unauthorized("signature verification failed".into()))
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

pub fn issue_jwt(secret: &str, address: &str) -> Result<String, AppError> {
    let claims = Claims {
        sub: address.to_string(),
        exp: (Utc::now() + Duration::seconds(JWT_TTL_SECS)).timestamp() as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(e.into()))
}

fn verify_jwt(secret: &str, token: &str) -> Result<String, AppError> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid or expired token".into()))?;
    Ok(data.claims.sub)
}

/// Axum extractor that resolves the authenticated wallet address from a
/// `Authorization: Bearer <jwt>` header.
pub struct AuthUser(pub String);

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
    String: FromRef<S>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| AppError::Unauthorized("missing bearer token".into()))?;
        let jwt_secret = String::from_ref(state);
        let address = verify_jwt(&jwt_secret, bearer.token())?;
        Ok(AuthUser(address))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn test_keypair() -> (SigningKey, String) {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let address = stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes())
            .to_string()
            .to_string();
        (signing_key, address)
    }

    #[test]
    fn valid_signature_over_challenge_verifies() {
        let (signing_key, address) = test_keypair();
        let message = challenge_message("abc123");
        let signature = signing_key.sign(&message);
        assert!(verify_wallet_signature(&address, &message, &signature.to_bytes()).is_ok());
    }

    #[test]
    fn signature_over_wrong_message_is_rejected() {
        let (signing_key, address) = test_keypair();
        let signature = signing_key.sign(&challenge_message("abc123"));
        let wrong_message = challenge_message("different-nonce");
        assert!(verify_wallet_signature(&address, &wrong_message, &signature.to_bytes()).is_err());
    }

    #[test]
    fn signature_from_wrong_key_is_rejected() {
        let (_, address) = test_keypair();
        let other_key = SigningKey::from_bytes(&[9u8; 32]);
        let message = challenge_message("abc123");
        let signature = other_key.sign(&message);
        assert!(verify_wallet_signature(&address, &message, &signature.to_bytes()).is_err());
    }

    #[test]
    fn challenge_round_trips_once_and_then_fails() {
        let store = ChallengeStore::new();
        let nonce = store.issue("GADDRESS");
        assert!(store.verify_and_consume("GADDRESS", &nonce));
        // Replay of the same nonce must fail: it was consumed.
        assert!(!store.verify_and_consume("GADDRESS", &nonce));
    }

    #[test]
    fn challenge_rejects_mismatched_nonce() {
        let store = ChallengeStore::new();
        store.issue("GADDRESS");
        assert!(!store.verify_and_consume("GADDRESS", "wrong-nonce"));
    }

    #[test]
    fn jwt_round_trips_and_rejects_wrong_secret() {
        let token = issue_jwt("secret-a", "GADDRESS").unwrap();
        assert_eq!(verify_jwt("secret-a", &token).unwrap(), "GADDRESS");
        assert!(verify_jwt("secret-b", &token).is_err());
    }
}
