# Authentication

alien-manager authenticates all API requests using Bearer tokens. Tokens are hashed before storage — the server never stores raw token values.

## Token Types

| Type | Prefix | Scope | Created By |
|------|--------|-------|------------|
| Admin | `ax_admin_` | Full access to all operations | Generated on first startup |
| Deployment Group | `ax_dg_` | Create deployments within the group | `POST /v1/deployment-groups/{id}/tokens` |
| Deployment | `ax_deploy_` | Single deployment: OTLP ingestion, command polling | Auto-created when a deployment is created with a DG token |

## Token Format

Each token is a 46-character string: a type prefix + 40 random hex characters.

```
ax_admin_a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2
└──────┘ └──────────────────────────────────────────┘
 prefix                  40 random hex chars
```

The prefix is stored separately for display purposes (e.g., showing `ax_admin_a1b2c3d4...` in admin UIs). The full token is only shown once at creation time.

## Hashed Storage

Tokens are stored as SHA-256 hashes. When a request arrives, alien-manager hashes the provided token and looks up the hash:

```sql
CREATE TABLE tokens (
  id                  TEXT PRIMARY KEY,
  type                TEXT NOT NULL,        -- "admin" | "deployment-group" | "deployment"
  key_prefix          TEXT NOT NULL,        -- first 12 chars for display
  key_hash            TEXT NOT NULL UNIQUE, -- SHA-256 hash of the full token
  deployment_group_id TEXT,                 -- set for DG and deployment tokens
  deployment_id       TEXT,                 -- set for deployment tokens
  project_id          TEXT NOT NULL,
  created_at          TEXT DEFAULT CURRENT_TIMESTAMP
)
```

Validation flow:
1. Extract `Bearer <token>` from `Authorization` header
2. Compute `SHA-256(token)`
3. Look up `key_hash` in the tokens table
4. If found, return the token's type and scope
5. If not found, return `401 Unauthorized`

## Token Lifecycle

**Admin token** — generated on first startup, printed to stdout once:

```
Admin API key: ax_admin_a1b2c3d4e5f6...
Save this key — it won't be shown again.
```

The raw token is never persisted — only its hash. If lost, generate a new one.

**Deployment group token** — created via `POST /v1/deployment-groups/{id}/tokens` with an admin token. The response includes the raw token; after this, the server only has the hash.

**Deployment token** — auto-created when a deployment is created using a DG token. Returned in the create-deployment response. Passed to the deployment as `ALIEN_TOKEN` for OTLP ingestion and command polling.

## Scope Enforcement

Each endpoint checks the caller's token type and scope:

| Operation | Admin | DG Token | Deployment Token |
|-----------|-------|----------|-----------------|
| Create deployment | Yes | Yes (own group) | No |
| List deployments | Yes | Yes (own group) | No |
| Delete deployment | Yes | No | No |
| Create release | Yes | No | No |
| Manage deployment groups | Yes | No | No |
| Send commands | Yes | Yes (own group) | No |
| Ingest logs/traces | Yes | No | Yes (own deployment) |
| Poll for commands | No | No | Yes (own deployment) |

## TokenStore Trait

```rust
#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn create_token(&self, params: CreateTokenParams) -> Result<TokenRecord>;
    async fn validate_token(&self, token_hash: &str) -> Result<Option<TokenRecord>>;
}
```

Default: `SqliteTokenStore` — stores hashed tokens in the SQLite database.

## AuthValidator Trait

```rust
#[async_trait]
pub trait AuthValidator: Send + Sync {
    async fn validate(&self, headers: &HeaderMap) -> Result<Option<AuthSubject>>;
}
```

Default: `TokenDbValidator` — extracts the Bearer token from headers, hashes it, and looks it up via `TokenStore`. Returns an `AuthSubject` with the caller's identity and scope.

## Dev Mode

In `alien dev`, authentication is permissive. Requests without a token fall back to a default admin-like scope rather than being rejected. No manual token bootstrap is required, and the CLI ensures the default `local-dev` deployment group exists after the embedded manager starts.
