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

## `localqueue.v1` — `LocalQueue`

Backed by a single SQLite database file at `<dataDir>/localqueue.sqlite`, where
`<dataDir>` is the directory passed to `LocalQueue::new` (for
`LocalQueue::from_binding` it is the binding's `queue_path`). The directory is
created if missing; the SQLite file (plus its `-wal`/`-shm` siblings) lives
inside it.

### Connection strategy

Identical to `localkv.v1`: `LocalQueue` stores **no** connection — only the
resolved file path. Every operation runs inside `tokio::task::spawn_blocking`
and opens its own short-lived connection, which is dropped before the task
returns. A connection is never held across an `.await`, and there is no
`Mutex<Connection>` anywhere. `LocalQueue` is consequently `Send + Sync` with
no interior locking. Correctness under concurrent access — including multiple
handles on the same file, i.e. multiple OS processes — is provided by SQLite:
**WAL** for concurrent readers alongside a single writer, and a 5s
**`busy_timeout`** so writers wait for the write lock instead of failing with
`SQLITE_BUSY`.

### DDL

```sql
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('format', 'localqueue.v1');

CREATE TABLE IF NOT EXISTS messages (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    payload_type   TEXT    NOT NULL,   -- "json" | "text"
    payload_data   TEXT    NOT NULL,   -- serialized JSON for "json", raw string for "text"
    enqueued_at    INTEGER NOT NULL,   -- unix epoch MILLISECONDS
    visible_at     INTEGER NOT NULL,   -- unix epoch MILLISECONDS; due when <= now
    attempt        INTEGER NOT NULL DEFAULT 0,  -- delivery count, incremented per receive
    receipt_handle TEXT                -- UUID of the CURRENT delivery, NULL if never delivered
);
CREATE INDEX IF NOT EXISTS idx_messages_visible ON messages (visible_at, enqueued_at, id);
```

Pragmas applied on every connection:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous  = NORMAL;
-- busy_timeout = 5000 ms (set via the rusqlite API)
```

### Versioning rule

The `meta` table carries the format identifier in the row
`('format', 'localqueue.v1')`. Any future incompatible change to the `messages`
schema MUST bump this string (e.g. `localqueue.v2`) and add explicit
migration/rejection logic. Readers MUST reject a format they do not understand
— and this implementation does: `LocalQueue::new` reads the marker after schema
init and fails fast (`BINDING_SETUP_FAILED`, naming both the found and the
supported format) unless it equals `localqueue.v1`. The `meta` row is written
with `INSERT OR IGNORE`, so re-opening an existing store never overwrites it.

### Receipt handles

- The **column** `receipt_handle` stores only the per-delivery **UUID** minted
  by the most recent receive of that row (`NULL` before the first delivery).
- The **caller-facing** receipt handle returned in `QueueMessage` is
  `"{id}:{uuid}"` — the row id and the column UUID joined by a single `:`.
  Other-language wrappers of this crate MUST treat it as opaque, but any reader
  of the store itself must know the split to correlate handles with rows.
- Each delivery mints a **fresh** UUID; redelivery therefore invalidates every
  previously issued handle for that message (they become *stale*).
- A handle that does not parse as `"{id}:{uuid}"` was never issued by this
  store; `ack`/`nack` treat it like an already-deleted message (no-op `Ok`).

### Semantics

- **Timestamps.** `enqueued_at`/`visible_at` are unix epoch time in
  **milliseconds**; `now` is `chrono::Utc::now().timestamp_millis()`. A message
  is *due* (deliverable) when `visible_at <= now`.

- **Send.** One insert; the message is immediately visible:

  ```sql
  INSERT INTO messages (payload_type, payload_data, enqueued_at, visible_at, attempt)
  VALUES (?1, ?2, ?3, ?3, 0);   -- ?3 = now
  ```

  Payloads over 64 KiB (serialized) are rejected before touching the store.

- **Receive.** The default visibility timeout is `LEASE_SECONDS` (30s). The
  pinned claim statement is, per message:

  ```sql
  UPDATE messages
  SET visible_at = ?1,             -- now + visibility timeout
      attempt = attempt + 1,
      receipt_handle = ?2          -- fresh UUID for THIS delivery
  WHERE id = ?3
  RETURNING payload_type, payload_data;
  ```

  A single bound parameter cannot mint a distinct UUID per claimed row, so a
  batch receive runs as **one `BEGIN IMMEDIATE` transaction**: select the due
  ids (`SELECT id FROM messages WHERE visible_at <= ?now ORDER BY enqueued_at,
  id LIMIT ?n` — `id` is the FIFO tiebreak for same-millisecond sends), run the
  claim statement above once per id, commit. `IMMEDIATE` acquires the write
  lock at `BEGIN`, so concurrent receivers across handles and processes
  serialize on the whole claim: each message is delivered to exactly one
  receiver per visibility window. If the transaction fails, the connection is
  dropped and SQLite rolls it back — no partial claims.

- **Ack.** Parse the handle into `(id, uuid)`, then in one transaction:

  ```sql
  DELETE FROM messages WHERE id = ?1 AND receipt_handle = ?2;
  ```

  - 1 row deleted → `Ok`.
  - 0 rows and the id no longer exists → **idempotent `Ok`** (already acked /
    purged — double-ack with the current receipt is a no-op).
  - 0 rows but the id still exists → the receipt is **stale** (the message was
    redelivered under a newer UUID) → **rejected** with
    `QUEUE_OPERATION_FAILED`. This prevents a slow consumer from deleting work
    that has since been handed to another consumer.

- **Nack.** Same receipt rules as ack; on a current receipt the message is made
  immediately visible (redeliverable now, keeping its `attempt` count):

  ```sql
  UPDATE messages SET visible_at = ?now WHERE id = ?1 AND receipt_handle = ?2;
  ```

- **Purge.** `DELETE FROM messages` — removes every row, visible or in flight.

- **`attempt`.** Starts at 0 on insert and is incremented by every successful
  claim, so it equals the number of deliveries so far (1 after the first
  receive, 2 after a redelivery, ...).

### Single implementation

`LocalQueue` (`src/providers/queue/local.rs`) is the **only** reader/writer of
`localqueue.v1`. There is no separate migration binary or alternate accessor;
the schema, pragmas, and the statements above are defined once in that file.
Any change to the on-disk contract must update both this document and that file
together (and bump the `meta` format string if incompatible).
