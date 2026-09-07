use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub soroban_rpc_url: String,
    pub escrow_contract_id: String,
    pub port: u16,
    pub indexer_poll_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set (e.g. postgres://user:pass@localhost/concord)"),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            soroban_rpc_url: env::var("SOROBAN_RPC_URL")
                .unwrap_or_else(|_| "https://soroban-testnet.stellar.org".to_string()),
            escrow_contract_id: env::var("ESCROW_CONTRACT_ID")
                .expect("ESCROW_CONTRACT_ID must be set to the deployed escrow contract address"),
            port: env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
            indexer_poll_interval_secs: env::var("INDEXER_POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
        }
    }
}
