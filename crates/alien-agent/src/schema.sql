-- Operator database schema (Turso with AEGIS-256 encryption)
--
-- Storage model:
-- - `state` table: Key-value for deployment_state, deployment_config, agent_id
-- - `approvals` table: Approval records (linked by release_id)
-- - `telemetry` table: Buffer for offline telemetry push

-- Key-value store for operator state
-- Stores:
-- - deployment_state (full DeploymentState JSON for step() input)
-- - deployment_config (DeploymentConfig JSON)
-- - agent_id (from initialization, for reference/logging)
CREATE TABLE IF NOT EXISTS state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Approvals history (for audit + dashboard)
CREATE TABLE IF NOT EXISTS approvals (
    id TEXT PRIMARY KEY,                      -- apr_xxx
    -- Release metadata
    -- JSON: { releaseId, version, description?, gitMetadata? }
    release_info TEXT,
    -- Snapshot of what was approved/rejected
    deployment_config TEXT NOT NULL,          -- DeploymentConfig JSON
    -- Decision
    status TEXT NOT NULL,                     -- pending, approved, rejected
    reason TEXT,                              -- Rejection reason
    created_at TEXT NOT NULL,
    decided_at TEXT,
    decided_by TEXT                           -- Who approved/rejected
);

-- Telemetry buffer for offline mode
CREATE TABLE IF NOT EXISTS telemetry (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,                       -- 'logs', 'metrics', 'traces'
    data BLOB NOT NULL,
    created_at TEXT NOT NULL
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_telemetry_created_at ON telemetry(created_at);
CREATE INDEX IF NOT EXISTS idx_approvals_status ON approvals(status);
