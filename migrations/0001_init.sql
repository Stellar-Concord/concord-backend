CREATE TABLE escrows (
    id              BIGINT PRIMARY KEY,
    contract_id     TEXT NOT NULL,
    client          TEXT NOT NULL,
    provider        TEXT NOT NULL,
    arbitrator      TEXT NOT NULL,
    token           TEXT NOT NULL,
    status          TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_ledger  BIGINT NOT NULL DEFAULT 0
);

CREATE INDEX idx_escrows_client ON escrows (client);
CREATE INDEX idx_escrows_provider ON escrows (provider);
CREATE INDEX idx_escrows_status ON escrows (status);

CREATE TABLE milestones (
    escrow_id       BIGINT NOT NULL REFERENCES escrows (id) ON DELETE CASCADE,
    milestone_id    INTEGER NOT NULL,
    description     TEXT NOT NULL,
    amount          NUMERIC(39, 0) NOT NULL,
    status          TEXT NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (escrow_id, milestone_id)
);

CREATE TABLE disputes (
    escrow_id               BIGINT NOT NULL REFERENCES escrows (id) ON DELETE CASCADE,
    milestone_id            INTEGER NOT NULL,
    raised_by               TEXT NOT NULL,
    reason                  TEXT NOT NULL,
    status                  TEXT NOT NULL,
    resolution_kind         TEXT,
    resolution_provider_bps INTEGER,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at             TIMESTAMPTZ,
    PRIMARY KEY (escrow_id, milestone_id)
);

CREATE INDEX idx_disputes_status ON disputes (status);

CREATE TABLE webhooks (
    id              UUID PRIMARY KEY,
    escrow_id       BIGINT NOT NULL REFERENCES escrows (id) ON DELETE CASCADE,
    owner_address   TEXT NOT NULL,
    url             TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_webhooks_escrow_id ON webhooks (escrow_id);

CREATE TABLE indexer_cursor (
    id                SMALLINT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    last_ledger       BIGINT NOT NULL DEFAULT 0,
    last_paging_token TEXT
);

INSERT INTO indexer_cursor (id, last_ledger, last_paging_token) VALUES (1, 0, NULL);
