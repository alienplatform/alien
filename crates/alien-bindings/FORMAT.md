# alien-bindings local store formats

On-disk formats for the SQLite-backed local development providers. These files
are the source of truth for the schema and semantics; the implementations must
match them exactly.

## `localkv.v1` — `LocalKv`

Backed by a single SQLite database file at `<dataDir>/localkv.sqlite`, where
`<dataDir>` is the directory passed to `LocalKv::new`. The directory is created
if missing; the SQLite file (plus its `-wal`/`-shm` siblings) lives inside it.

### Connection strategy

`rusqlite::Connection` is `Send` but not `Sync`, and every call is blocking.
`LocalKv` therefore stores **no** connection — only the resolved file path. Every
operation runs inside `tokio::task::spawn_blocking` and opens its own short-lived
connection, which is dropped before the task returns. A connection is never held
across an `.await`, and there is no `Mutex<Connection>` anywhere. `LocalKv` is
consequently `Send + Sync` with no interior locking.

Correctness under concurrent access — including multiple `LocalKv` handles on the
same file, i.e. multiple OS processes — is provided by SQLite:

- **WAL** (`PRAGMA journal_mode=WAL`) allows concurrent readers alongside a
  single writer.
- **`busy_timeout`** (5s) makes a writer wait for the write lock instead of
  failing with `SQLITE_BUSY`.

The schema is created once in `LocalKv::new`; per-operation connections only set
the connection-scoped pragmas (`journal_mode`, `synchronous`, `busy_timeout`),
so reads never take the write lock.

### DDL

```sql
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('format', 'localkv.v1');

CREATE TABLE IF NOT EXISTS kv (
    key        TEXT PRIMARY KEY,
    value      BLOB NOT NULL,
    expires_at INTEGER          -- unix epoch MILLISECONDS, NULL = no expiry
);
```

Pragmas applied on every connection:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
-- busy_timeout = 5000 ms (set via the rusqlite API)
```

### Versioning rule

The `meta` table carries the format identifier in the row
`('format', 'localkv.v1')`. Any future incompatible change to the `kv` schema
MUST bump this string (e.g. `localkv.v2`) and add explicit migration/rejection
logic. Readers MUST reject a format they do not understand — and this
implementation does: `LocalKv::new` reads the marker after schema init and fails
fast (`BINDING_SETUP_FAILED`, naming both the found and the supported format)
unless it equals `localkv.v1`. The `meta` row is written with `INSERT OR
IGNORE`, so re-opening an existing store never overwrites it.

### Semantics

- **Timestamps.** `expires_at` is unix epoch time in **milliseconds**. `now` is
  `chrono::Utc::now().timestamp_millis()`. A row is expired when
  `expires_at IS NOT NULL AND expires_at <= now`.

- **Lazy expiry.** Expired rows read as absent. `get` and `exists` delete the
  expired row they encounter (`DELETE ... WHERE key = ? AND expires_at IS NOT
  NULL AND expires_at <= ?now`) before reporting absence. `scan_prefix` filters
  expired rows out of its results but does not delete them. Physical deletion is
  therefore eventual, matching the `Kv` trait's soft-hint TTL contract.

- **Unconditional put.** Upsert in one statement:

  ```sql
  INSERT INTO kv (key, value, expires_at) VALUES (?1, ?2, ?3)
  ON CONFLICT(key) DO UPDATE SET value = ?2, expires_at = ?3;
  ```

  Always returns `true`.

- **Conditional put (`if_not_exists`).** One atomic statement; the winner is
  detected via `changes()` (the row count returned by `execute`):

  ```sql
  INSERT INTO kv (key, value, expires_at) VALUES (?1, ?2, ?3)
  ON CONFLICT(key) DO UPDATE SET value = ?2, expires_at = ?3
  WHERE kv.expires_at IS NOT NULL AND kv.expires_at <= ?4;   -- ?4 = now
  ```

  - Key absent → `INSERT` runs → `changes() == 1` → **win** (returns `true`).
  - Key present but expired → the `DO UPDATE ... WHERE` matches → overwrite →
    `changes() == 1` → **win** (takeover of an expired key).
  - Key present and live → the `WHERE` fails, so the conflict resolves to a
    no-op → `changes() == 0` → **lose** (returns `false`).

  Because SQLite serializes writers, when N callers (across any number of
  handles/processes) race this statement on the same key, exactly one observes
  `changes() == 1`.

- **Scan.** `scan_prefix` selects `WHERE key >= ?prefix ORDER BY key`, stops at
  the first key that no longer starts with the prefix, filters expired rows, and
  paginates with a simple 0-based integer offset cursor. Results are ordered by
  key. An unparseable cursor returns `INVALID_INPUT`.

### Single implementation

`LocalKv` (`src/providers/kv/local.rs`) is the **only** reader/writer of
`localkv.v1`. There is no separate migration binary or alternate accessor; the
schema, pragmas, and the statements above are defined once in that file. Any
change to the on-disk contract must update both this document and that file
together (and bump the `meta` format string if incompatible).
