# alien-bindings local store formats

On-disk formats for the local development providers. These files are the
source of truth for the schema and semantics; the implementations must match
them exactly.

## Engine

Both stores run on **turso**, an async-native database engine that reads and
writes the **SQLite-compatible file format**. Standard SQLite tooling can
inspect the store after all processes using it have stopped. Compatible
database files do not imply interchangeable live WAL coordination.
Live queue inspection uses the owner's HTTP metrics endpoint; no observer
opens the queue database.

Deployed queues and the manager's command KV use single-process ownership.
General local KV bindings retain the experimental multi-process WAL mode
(`Builder::experimental_multiprocess_wal(true)`) on 64-bit Unix platforms.
This change does not alter KV ownership or its format. The exact Turso version
remains pinned because WAL coordination can change between engine releases.

Alongside the database file Turso maintains WAL sidecar files (e.g. `-wal`);
preserve every `<file>.sqlite*` sibling when backing up or migrating a store.

## `localkv.v2` — `LocalKv`

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
INSERT OR IGNORE INTO meta (key, value) VALUES ('format', 'localkv.v2');

CREATE TABLE IF NOT EXISTS kv (
    key        TEXT PRIMARY KEY,
    value      BLOB NOT NULL,
    expires_at INTEGER,         -- unix epoch MILLISECONDS, NULL = no expiry
    version    TEXT NOT NULL    -- changes on every applied put
);
```

Settings: multi-process WAL mode for general KV bindings (single-process mode
for the manager's command KV) and a 5s `busy_timeout` on each connection.

### Versioning rule

The `meta` table carries the format identifier in the row
`('format', 'localkv.v2')`. Any future incompatible change to the `kv` schema
MUST bump this string and add explicit migration/rejection
logic. Readers MUST reject a format they do not understand — and this
implementation does: `LocalKv::new` checks the marker before creating any
provider tables and fails fast (`BINDING_SETUP_FAILED`, naming both the found
and the supported format) unless it equals `localkv.v2`, so a rejected foreign
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

- **Versions.** Every applied put writes a fresh random version. Reads wrap that
  backend version in an opaque, key-bound token. Callers may pass the token to
  a conditional put or delete; tokens must never be parsed or modified.

- **Unconditional put.** Upsert in one statement:

  ```sql
  INSERT INTO kv (key, value, expires_at, version) VALUES (?1, ?2, ?3, ?4)
  ON CONFLICT(key) DO UPDATE
  SET value = ?2, expires_at = ?3, version = ?4;
  ```

  Always returns `true`.

- **Conditional put (absent).** One atomic statement; the winner is
  detected via the changed-row count returned by `execute`:

  ```sql
  INSERT INTO kv (key, value, expires_at, version) VALUES (?1, ?2, ?3, ?4)
  ON CONFLICT(key) DO UPDATE
  SET value = ?2, expires_at = ?3, version = ?4
  WHERE kv.expires_at IS NOT NULL AND kv.expires_at <= ?5;   -- ?5 = now
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

- **Conditional put (version).** A single `UPDATE` matches the key, stored
  version, and live TTL. It writes the new value, TTL, and a fresh version.
  The changed-row count is `1` only for the winner.

- **Conditional delete.** A single `DELETE` matches the key, stored version,
  and live TTL. A mismatch, missing row, or logically expired row returns
  `false`; a matching delete returns `true`.

- **Scan.** `scan_prefix` selects `WHERE key >= ?prefix ORDER BY key`, stops at
  the first key that no longer starts with the prefix, filters expired rows, and
  returns each entry's opaque version. Pagination resumes strictly after the
  last returned key using a prefix-bound opaque cursor. Results are ordered by
  key. An invalid or cross-prefix cursor returns `INVALID_INPUT`.

### Single implementation

`LocalKv` (`src/providers/kv/local.rs`) is the **only** reader/writer of
`localkv.v2`. There is no separate migration binary or alternate accessor;
the schema, settings, and the statements above are defined once in that file
(the shared open/init handshake lives in `src/providers/local_store.rs`).
Existing stores with another format marker, including `localkv.v1`, are
rejected. Any change to the on-disk contract must update both this document
and that file together (and bump the `meta` format string if incompatible).

## `localqueue.v1` — `LocalQueue`

Backed by a single database file at `<dataDir>/localqueue.sqlite`, where
`<dataDir>` is the directory passed to `LocalQueue::new` (for
`LocalQueue::from_binding` it is the binding's `queue_path`). The directory is
created if missing; the database file (plus its WAL siblings) lives inside it.

### Connection strategy

Local deployments use a single owner in the existing manager (or local Operator).
The `local-queue-http` binding points directly at that owner's existing HTTP listener;
Containers and Daemons still run their application process without a runtime wrapper.
Workers, triggers, health checks and HTTP observers share the same queue handle.
Queue files are never mounted into application containers.

The owner opens Turso in single-process mode and serializes operations on a revocable
handle. Each operation uses a fresh connection, drains statements and awaits commit
before returning success. The pinned engine defaults to synchronous FULL. Deletion
waits for active operations, revokes old handles, then removes the directory.

Local KV retains its existing multiprocess implementation. Its shared store plumbing
already supports single-process mode; queue ownership does not change the KV format.

### Compatibility and offline upgrade

Older SDKs do not recognize `local-queue-http` and fail binding initialization. Current
SDKs reject legacy `local-queue` file bindings before opening or creating files. Rebuild
all applications, including native Workers and container images, with the matching SDK.
There is no file-access fallback or automatic request retry.

The owner stores queues under `<stateDir>/queue-owned/<resourceId>`. A legacy
`<stateDir>/queue/<resourceId>` directory blocks opening that queue, even if an owned
directory also exists. This deliberately prevents an upgrade from overlapping clients
that still use mounted files. A manager restart alone does not stop standalone Docker
workloads and therefore cannot authorize migration.

To preserve an existing local queue:

1. Stop the previous manager/Operator and every producer, consumer and observer. Stop
   the deployment's Docker workloads too. Do not migrate while an older client holds
   the directory or can restart itself.
2. Back up the entire queue directory, including its database, WAL and shared-memory
   files together. Do not copy just the SQLite file or edit pending rows.
3. With all clients stopped, move the entire `queue/<resourceId>` directory into
   `queue-owned/<resourceId>` under the same state directory. The destination must not
   already exist. The `localqueue.v1` SQL format is unchanged; no rows are rewritten.
4. Rebuild applications with the current SDK. Remove the stopped, deployment-owned
   Docker containers (preserve data directories), so reconciliation recreates them with
   HTTP bindings instead of retaining their old mount/environment configuration.
5. Restart the current manager/Operator with the same deployment state and reconcile
   the deployment. Native workloads receive fresh runtime bindings. If an earlier
   upgrade attempt left a failed resource, retry that deployment after the handoff.

Downgrading requires stopping all clients and an explicit reverse offline handoff;
never run old and new owners together. Direct database access is not a supported queue
observer interface. Observers use the authenticated `stats` HTTP operation (or
`LocalHttpQueue::stats`) to read total, visible, in-flight and delayed counts.

Queue credentials are scoped per queue and persisted in a mode-0600 local file. They
are injected into runtime bindings, omitted from saved controller/worker state, and
checked by hash. Requests go to `POST <binding.endpoint>` with the binding token as a
Bearer credential. The JSON `operation` selects send, sendBatch, receive, ack, ackBatch,
nack, renew, purge or stats. Transport failures leave acceptance uncertain; batch
responses preserve one outcome per input and never claim an uncertain send succeeded.

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

Settings applied by the owner: single-process WAL mode and a 5s `busy_timeout`
on each connection.

### Versioning rule

The `meta` table carries the format identifier in the row
`('format', 'localqueue.v1')`. Any future incompatible change to the `messages`
schema MUST bump this string (e.g. `localqueue.v2`) and add explicit
migration/rejection logic. Readers MUST reject a format they do not understand
— and this implementation does: `LocalQueue` reads the marker before provider schema
initialization and fails fast (`BINDING_SETUP_FAILED`, naming both the found and the
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
