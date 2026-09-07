use crate::auth::ChallengeStore;
use axum::extract::FromRef;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub jwt_secret: String,
    pub challenges: Arc<ChallengeStore>,
}

impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl FromRef<AppState> for String {
    fn from_ref(state: &AppState) -> Self {
        state.jwt_secret.clone()
    }
}

impl FromRef<AppState> for Arc<ChallengeStore> {
    fn from_ref(state: &AppState) -> Self {
        state.challenges.clone()
    }
}
