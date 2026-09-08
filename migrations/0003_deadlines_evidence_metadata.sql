-- Deadlines/expiration, timeout auto-release, mutual cancellation,
-- milestone evidence, and escrow metadata all landed as contract features
-- with no backend-side rows to backfill (existing indexed data predates
-- this and is about to be superseded by a fresh contract deployment
-- anyway) -- so these columns are nullable rather than backfilled, and the
-- indexer populates them going forward.

ALTER TABLE escrows
    ADD COLUMN review_period    BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN chain_created_at TIMESTAMPTZ,
    ADD COLUMN title            TEXT,
    ADD COLUMN metadata_uri     TEXT,
    ADD COLUMN metadata_hash    TEXT;

ALTER TABLE milestones
    ADD COLUMN deadline      TIMESTAMPTZ,
    ADD COLUMN submitted_at  TIMESTAMPTZ,
    ADD COLUMN evidence_uri  TEXT,
    ADD COLUMN evidence_hash TEXT,
    -- 'approved' (client called approve_milestone) or 'auto_release'
    -- (timeout auto-release fired). NULL until the milestone is released.
    ADD COLUMN released_via  TEXT;
