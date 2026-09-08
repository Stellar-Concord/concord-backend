# concord-backend

Event indexer and read API for [Concord](../concord-contracts)'s escrow
contract. Polls Soroban RPC for the contract's events, reconstructs
escrow/milestone/dispute state into Postgres, and serves it over a small
REST API. Writes never go through this service — [`concord-frontend`](../concord-frontend)
talks to the contract directly with a wallet-signed transaction; this is
purely the read side plus webhook notifications.

```
Soroban RPC ──(poll getEvents)──▶ indexer ──▶ Postgres ──▶ REST API ──▶ frontend / integrators
                                      │
                                      └──▶ signed webhook POSTs on every state change
```

## Setup

Requires Rust and a Postgres server.

```bash
cp .env.example .env   # then edit DATABASE_URL / JWT_SECRET
cargo run
```

Migrations in `migrations/` run automatically on startup. See
[`.env.example`](./.env.example) for all config (`DATABASE_URL`,
`JWT_SECRET`, `SOROBAN_RPC_URL`, `ESCROW_CONTRACT_ID`, `PORT`,
`INDEXER_POLL_INTERVAL_SECS`). `ESCROW_CONTRACT_ID` defaults to the current
testnet deployment — see
[`../concord-contracts/DEPLOYMENTS.md`](../concord-contracts/DEPLOYMENTS.md).

## API

All reads are public; only webhook registration/management requires auth.

| | | |
|---|---|---|
| `GET` | `/escrows` | List escrows. Filter with `?client=`, `?provider=`, `?status=`, paginate with `?limit=` |
| `GET` | `/escrows/:id` | One escrow with its milestones |
| `GET` | `/disputes` | List disputes. `?status=open` (default), `resolved`, or `all` |
| `POST` | `/auth/challenge` | `{ address }` → `{ nonce, message }` |
| `POST` | `/auth/verify` | `{ address, nonce, signature }` → `{ token }` (JWT) |
| `POST` | `/escrows/:id/webhooks` | *Auth.* Register a webhook. Returns the signing secret **once** |
| `GET` | `/escrows/:id/webhooks` | *Auth.* List your own webhooks on this escrow (secret never included) |
| `DELETE` | `/escrows/:id/webhooks/:webhook_id` | *Auth, owner only.* Remove a webhook |

### Wallet auth

No passwords. A client requests a nonce for their Stellar address, signs it
with their keypair, and exchanges the signature for a short-lived JWT:

```
POST /auth/challenge  { "address": "G..." }
  → { "nonce": "...", "message": "Concord auth challenge: <nonce>" }

# sign `message` with the account's Stellar keypair, base64-encode the signature

POST /auth/verify  { "address": "G...", "nonce": "...", "signature": "<base64>" }
  → { "token": "<jwt>" }
```

Use the token as `Authorization: Bearer <token>` on the webhook endpoints.
Only the escrow's client, provider, or arbitrator may register a webhook
for it.

### Webhook delivery

On every indexed event for an escrow with registered webhooks, the indexer
`POST`s `{ "escrow_id": ..., "event": "<event_name>" }` to each URL, signed
so the receiver can verify it actually came from here:

```
x-concord-timestamp: <unix seconds>
x-concord-signature: sha256=<hmac-sha256("{timestamp}.{body}", your webhook's secret)>
```

The secret is shown exactly once, in the registration response — store it
then, there's no way to retrieve it again. Including the timestamp in the
signed content lets you reject stale replays of an old (legitimately
signed) payload.

## Project layout

```
src/
├── main.rs            # wiring: config, DB pool, indexer task, axum server
├── config.rs          # env var loading
├── state.rs           # AppState + FromRef impls for axum extractors
├── db.rs               # every SQL query lives here
├── models.rs           # row/response types
├── error.rs            # AppError -> HTTP response mapping
├── auth.rs              # challenge/verify, JWT issue/verify, wallet signature check
├── api/
│   ├── mod.rs          # route table
│   ├── escrows.rs, disputes.rs, webhooks.rs, auth.rs
│   └── tests.rs        # HTTP-layer integration tests
└── indexer/
    ├── mod.rs          # poll loop, event dispatch, webhook delivery
    ├── rpc.rs          # Soroban RPC client (getEvents, getLatestLedger)
    └── decode.rs        # XDR -> typed event decoding
```

## Testing

```bash
DATABASE_URL=postgres://user:pass@localhost/postgres cargo test
```

The indexer, auth, and RPC-decoding logic are plain unit tests (no DB
needed). The HTTP route handlers (`api/tests.rs`) use
[`#[sqlx::test]`](https://docs.rs/sqlx/latest/sqlx/attr.test.html), which
creates, migrates, and drops an isolated Postgres database per test — point
`DATABASE_URL` at any server you can create databases on (a local one is
fine; nothing needs to exist there beforehand).

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
```

CI (`.github/workflows/ci.yml`) runs all of the above against a
`postgres:16` service container on every push and PR.

## Notes on what's verified vs. not

The indexer's event decoding was validated against a *real* Soroban RPC
response captured from testnet (not just hand-built fixtures) — running
against a live RPC surfaced two real bugs (`startLedger: 0` is rejected;
the per-event cursor field is `id`, not `pagingToken`) that are now fixed
and regression-tested. See
[`../concord-contracts/DEPLOYMENTS.md`](../concord-contracts/DEPLOYMENTS.md)
for the full live-verification notes.
