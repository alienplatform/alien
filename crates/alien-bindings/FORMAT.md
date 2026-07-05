# alien-bindings local store formats

On-disk formats for the local development providers. These files are the
source of truth for the schema and semantics; the implementations must match
them exactly.

## Engine

Both stores run on **turso**, an async-native database engine that reads and
writes the **SQLite-compatible file format** — the `.sqlite` file extension
stays truthful, and the files can be inspected with standard SQLite tooling.

Multi-process safety comes from turso's **multi-process WAL mode**
(`Builder::experimental_multiprocess_wal(true)`), enabled explicitly on every
open. Honest caveats, straight from turso's own positioning of the feature:

- The mode is **experimental** upstream. Its cross-process WAL coordination
  format may change between turso releases, which is why the crate pins an
  exact turso version.
- It targets 64-bit Unix platforms.

Both trade-offs are acceptable here: these are local development stores with
disposable state, and the multi-handle concurrency tests in the provider
modules are the gate that the mode actually delivers the semantics pinned
below. Alongside the database file turso maintains WAL sidecar files (e.g.
`-wal`); treat every `<file>.sqlite*` sibling as part of the store.

## `localkv.v1` — `LocalKv`

Backed by a single database file at `<dataDir>/localkv.sqlite`, where
`<dataDir>` is the directory passed to `LocalKv::new`. The directory is created
if missing; the database file (plus its WAL siblings) lives inside it.

### Connection strategy

turso is async-native and its `Connection` is `Send + Sync`, so there is no
blocking boundary and no `Mutex<Connection>` anywhere. `LocalKv` holds one
`turso::Database` handle; every operation opens its own short-lived connection
from it, which is dropped when the operation completes. Statements are always
driven to completion (queries drained until exhausted) — an unfinished turso
statement keeps its implicit transaction open, which would block writers and
freeze read snapshots.

Correctness under concurrent access — including multiple `LocalKv` handles on
the same file, i.e. multiple OS processes — is provided by the engine:

- **Multi-process WAL mode** (see *Engine* above) coordinates readers and
  writers across processes.
- **`busy_timeout`** (5s) makes a writer wait for the write lock instead of
  failing with `Busy`.

The schema is created once in `LocalKv::new`. Reads of live rows never take
the write lock; a read that encounters an expired row escalates to a short
delete.

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

Settings applied per open: multi-process WAL mode (on the database handle) and
a 5s `busy_timeout` (on every connection, via the turso API).

### Versioning rule

The `meta` table carries the format identifier in the row
`('format', 'localkv.v1')`. Any future incompatible change to the `kv` schema
MUST bump this string (e.g. `localkv.v2`) and add explicit migration/rejection
logic. Readers MUST reject a format they do not understand — and this
implementation does: `LocalKv::new` checks the marker before creating any
provider tables and fails fast (`BINDING_SETUP_FAILED`, naming both the found
and the supported format) unless it equals `localkv.v1`, so a rejected foreign
store is left untouched. The `meta` row is written with `INSERT OR
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
  detected via the changed-row count returned by `execute`:

  ```sql
  INSERT INTO kv (key, value, expires_at) VALUES (?1, ?2, ?3)
  ON CONFLICT(key) DO UPDATE SET value = ?2, expires_at = ?3
  WHERE kv.expires_at IS NOT NULL AND kv.expires_at <= ?4;   -- ?4 = now
  ```

  - Key absent → `INSERT` runs → changed count `== 1` → **win** (returns
    `true`).
  - Key present but expired → the `DO UPDATE ... WHERE` matches → overwrite →
    changed count `== 1` → **win** (takeover of an expired key).
  - Key present and live → the `WHERE` fails, so the conflict resolves to a
    no-op → changed count `== 0` → **lose** (returns `false`).

  Because the engine serializes writers, when N callers (across any number of
  handles/processes) race this statement on the same key, exactly one observes
  a changed count of 1.

- **Scan.** `scan_prefix` selects `WHERE key >= ?prefix ORDER BY key`, stops at
  the first key that no longer starts with the prefix, filters expired rows, and
  paginates with a simple 0-based integer offset cursor. Results are ordered by
  key. An unparseable cursor returns `INVALID_INPUT`.

### Single implementation

`LocalKv` (`src/providers/kv/local.rs`) is the **only** reader/writer of
`localkv.v1`. There is no separate migration binary or alternate accessor; the
schema, settings, and the statements above are defined once in that file (the
shared open/init handshake lives in `src/providers/local_store.rs`). Any
change to the on-disk contract must update both this document and that file
together (and bump the `meta` format string if incompatible).

## `localqueue.v1` — `LocalQueue`

Backed by a single database file at `<dataDir>/localqueue.sqlite`, where
`<dataDir>` is the directory passed to `LocalQueue::new` (for
`LocalQueue::from_binding` it is the binding's `queue_path`). The directory is
created if missing; the database file (plus its WAL siblings) lives inside it.

### Connection strategy

Identical to `localkv.v1`: `LocalQueue` holds one `turso::Database` handle;
every operation opens its own short-lived connection from it, which is dropped
when the operation completes, and every statement is driven to completion. No
connection crosses operations and there is no `Mutex<Connection>` anywhere.
Correctness under concurrent access — including multiple handles on the same
file, i.e. multiple OS processes — is provided by the engine's multi-process
WAL mode (see *Engine* above) and a 5s **`busy_timeout`** so writers wait for
the write lock instead of failing with `Busy`.

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

Settings applied per open: multi-process WAL mode (on the database handle) and
a 5s `busy_timeout` (on every connection, via the turso API).

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
  RETURNING payload_type, payload_data, attempt;
  ```

  The returned `attempt` (post-increment, so 1-based) is surfaced to the
  caller on the delivered message, so consumers can enforce retry limits.

  A single bound parameter cannot mint a distinct UUID per claimed row, so a
  batch receive runs as **one `BEGIN IMMEDIATE` transaction**: select the due
  ids (`SELECT id FROM messages WHERE visible_at <= ?now ORDER BY enqueued_at,
  id LIMIT ?n` — `id` is the FIFO tiebreak for same-millisecond sends), run the
  claim statement above once per id, commit. `IMMEDIATE` acquires the write
  lock at `BEGIN`, so concurrent receivers across handles and processes
  serialize on the whole claim: each message is delivered to exactly one
  receiver per visibility window. If the transaction fails, it is rolled back
  — no partial claims.

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
the schema, settings, and the statements above are defined once in that file
(the shared open/init handshake lives in `src/providers/local_store.rs`). Any
change to the on-disk contract must update both this document and that file
together (and bump the `meta` format string if incompatible).
