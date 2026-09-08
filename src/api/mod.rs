mod auth;
mod disputes;
mod escrows;
#[cfg(test)]
mod tests;
mod webhooks;

use crate::state::AppState;
use axum::routing::{delete, get, post};
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/challenge", post(auth::challenge))
        .route("/auth/verify", post(auth::verify))
        .route("/escrows", get(escrows::list_escrows))
        .route("/escrows/{id}", get(escrows::get_escrow))
        .route("/disputes", get(disputes::list_disputes))
        .route(
            "/escrows/{id}/webhooks",
            post(webhooks::register_webhook).get(webhooks::list_webhooks),
        )
        .route(
            "/escrows/{id}/webhooks/{webhook_id}",
            delete(webhooks::delete_webhook),
        )
}
