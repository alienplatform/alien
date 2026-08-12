/**
 * Public types for `@alienplatform/bindings`: the app-facing resource handle
 * interfaces and their operation option/result shapes. These mirror the Rust
 * `alien-bindings` handles; the native (napi) surface is an internal detail.
 */

/** Metadata for a single stored object. */
export interface ObjectMeta {
  /** Object location (path) within the store. */
  location: string
  /** Object size in bytes. */
  size: number
  /** Last-modified timestamp as an RFC 3339 string. */
  lastModified: string
  /** Provider entity tag, when available. */
  eTag?: string
  /** Provider object version, when available. */
  version?: string
}

/** HTTP method a presigned request may be issued for. */
export type SignedUrlMethod = "GET" | "PUT" | "DELETE"

/** Options for {@link Storage.signedUrl}. */
export interface SignedUrlOptions {
  /** Which operation the presigned request authorizes. */
  method: SignedUrlMethod
  /** Object path within the store. */
  path: string
  /** Validity window, in seconds. */
  expiresIn: number
}

/**
 * A presigned request: the URL plus the method and headers to replay it with.
 * This is a full request description (not a bare URL) so it matches every
 * provider, including local stores whose URL is a `local://` scheme.
 */
export interface PresignedRequest {
  url: string
  method: string
  headers: Record<string, string>
}

/** Provider-neutral attributes returned with a stored object. */
export interface StorageObjectAttributes {
  /** Stored MIME type. */
  contentType?: string
  /** Stored browser content-disposition behavior. */
  contentDisposition?: string
  /** Stored content encoding. */
  contentEncoding?: string
  /** Stored content language. */
  contentLanguage?: string
  /** Stored cache-control policy. */
  cacheControl?: string
  /** Provider storage class, when reported. */
  storageClass?: string
  /** User-defined object metadata. */
  metadata: Record<string, string>
}

/** Provider-neutral object attributes accepted by {@link Storage.put}. */
export interface StoragePutAttributes {
  /** MIME type to store with the object. */
  contentType?: string
  /** Browser content-disposition behavior to store with the object. */
  contentDisposition?: string
  /** Content encoding to store. GCS rejects `gzip` because it transcodes gzip responses. */
  contentEncoding?: string
  /** Content language to store with the object. */
  contentLanguage?: string
  /** Cache-control policy to store with the object. */
  cacheControl?: string
  /** User-defined object metadata to store. */
  metadata?: Record<string, string>
}

/** Options for {@link Storage.put}. */
export interface StoragePutOptions {
  attributes?: StoragePutAttributes
}

/** Result of reading a stored object. */
export interface StorageGetResult {
  data: Buffer
  meta: ObjectMeta
  attributes: StorageObjectAttributes
}

/** Result of reading object information without its payload. */
export interface StorageHeadResult {
  meta: ObjectMeta
  attributes: StorageObjectAttributes
}

/** Provider identifiers returned after a successful storage write. */
export interface StoragePutResult {
  eTag?: string
  version?: string
}

/** A resolved object-storage binding. */
export interface Storage {
  /** Fetch the object at `path`. */
  get(path: string): Promise<StorageGetResult>
  /** Store `data` at `path`, optionally with provider-neutral object attributes. */
  put(
    path: string,
    data: Buffer | Uint8Array,
    options?: StoragePutOptions,
  ): Promise<StoragePutResult>
  /** Delete the object at `path`. */
  delete(path: string): Promise<void>
  /** List objects, optionally filtered by `prefix`. */
  list(prefix?: string): Promise<ObjectMeta[]>
  /** Fetch metadata and attributes for `path` without downloading its payload. */
  head(path: string): Promise<StorageHeadResult>
  /** Copy the object at `from` to `to`. */
  copy(from: string, to: string): Promise<void>
  /** Create a presigned request for `path`. */
  signedUrl(options: SignedUrlOptions): Promise<PresignedRequest>
}

/** Optional authenticated metadata for a Key operation. */
export interface KeyOptions {
  context?: Record<string, string>
}

/** A provider-backed key for encrypting and decrypting values up to 128 bytes. */
export interface Key {
  encrypt(plaintext: Buffer | Uint8Array, options?: KeyOptions): Promise<Buffer>
  decrypt(ciphertext: Buffer | Uint8Array, options?: KeyOptions): Promise<Buffer>
}

/** Storage operations available from an external deployment binding. */
export type RemoteStorage = Pick<Storage, "get" | "put" | "delete" | "list" | "head">

/** Options for {@link Kv.set}. */
export interface KvSetOptions {
  /** Time-to-live, in seconds. */
  ttl?: number
  /**
   * Atomic write precondition. `null` means the key must be absent; an opaque
   * version means the key must still match an earlier read. Omit for an
   * unconditional write.
   */
  ifVersion?: string | null
}

/** Options for {@link Kv.delete}. */
export interface KvDeleteOptions {
  /** Delete only when the key still matches this opaque version. */
  ifVersion?: string
}

/** A value and its opaque version. */
export interface KvEntry<T> {
  /** The key. */
  key: string
  /** The decoded value. */
  value: T
  /** Opaque version for a later conditional set or delete. */
  version: string
}

/** A raw entry returned by a scan. */
export type KvScanItem = KvEntry<Buffer>

/** A page of scan results. */
export interface KvScanResult {
  /**
   * Key-value pairs found on this page. Values are returned alongside their
   * keys (the provider already reads them), so a scan needs no follow-up `get`.
   */
  items: KvScanItem[]
  /** Opaque cursor for the next page, or `undefined` when exhausted. */
  nextCursor?: string
}

/** A resolved key-value binding. */
export interface Kv {
  /** Get the raw entry for `key`, or `null` if absent/expired. */
  get(key: string): Promise<KvEntry<Buffer> | null>
  /** Get the entry for `key` with its value decoded as UTF-8 text. */
  getText(key: string): Promise<KvEntry<string> | null>
  /** Get the entry for `key` with its value parsed as JSON. */
  getJson<T = unknown>(key: string): Promise<KvEntry<T> | null>
  /**
   * Set `key` to the UTF-8 `value`. Conditional writes resolve `false` when
   * their version precondition is not met; all applied writes resolve `true`.
   */
  set(key: string, value: string, options?: KvSetOptions): Promise<boolean>
  /**
   * Set `key` to `value` serialized as JSON (via `JSON.stringify`). Conditional
   * writes resolve `false` when their version precondition is not
   * met; all applied writes resolve `true`.
   */
  setJson(key: string, value: unknown, options?: KvSetOptions): Promise<boolean>
  /** Delete `key`, optionally only when its version still matches. */
  delete(key: string, options?: KvDeleteOptions): Promise<boolean>
  /** Check whether `key` exists. */
  exists(key: string): Promise<boolean>
  /** Scan keys under `prefix`, with optional pagination. */
  scan(prefix: string, limit?: number, cursor?: string): Promise<KvScanResult>
}

/** A message received from a queue. */
export interface QueueMessage {
  /** Payload discriminant: `"json"` or `"text"`. */
  payloadType: "json" | "text"
  /**
   * The payload string: serialized JSON when `payloadType === "json"`, raw text
   * when `payloadType === "text"`.
   */
  payload: string
  /** Opaque receipt handle for ack/nack. */
  receiptHandle: string
  /**
   * Delivery attempt, 1-based (1 = first delivery). Providers that do not report
   * redelivery counts always set 1; use it to enforce retry limits.
   */
  attempt: number
}

/** A resolved queue binding. */
export interface Queue {
  /** Send a JSON message (the object is serialized with `JSON.stringify`). */
  send(message: unknown): Promise<void>
  /** Send a raw text message. */
  sendText(text: string): Promise<void>
  /** Receive up to `max` messages. */
  receive(max: number): Promise<QueueMessage[]>
  /** Acknowledge a message by its receipt handle. */
  ack(receipt: string): Promise<void>
  /** Negative-acknowledge a message, making it immediately redeliverable. */
  nack(receipt: string): Promise<void>
  /** Delete every message in the queue. */
  purge(): Promise<void>
}

/** Read-only service discovery for a linked container. */
export interface Container {
  /** Get the URL reachable from the deployment's private network. */
  getInternalUrl(): Promise<string>
  /** Get the public URL when the container is publicly exposed. */
  getPublicUrl(): Promise<string | null>
}

/**
 * TLS mode for a Postgres connection: `disable` for the local developer backend
 * or an explicit BYO plaintext opt-out, `verify-full` for a BYO database with
 * certificate and hostname verification, `verify-ca` for Cloud SQL over its
 * Private Service Connect IP, and `verify-full` for Aurora and Flexible Server.
 */
export type PostgresSslMode = "disable" | "verify-ca" | "verify-full"

/** TLS options for `verify-ca`, where the provider CA authenticates the server. */
interface PostgresVerifyCaTlsOptions {
  /**
   * One or more PEM-encoded provider CA certificates. `verify-ca` cannot use the
   * operating system trust store because the certificate is specific to the instance.
   */
  ca: [string, ...string[]]
  /** Always true: an untrusted certificate fails the connection. */
  rejectUnauthorized: true
  /**
   * Skip hostname matching after the CA chain succeeds because Cloud SQL's
   * certificate does not contain the Private Service Connect IP being dialed.
   */
  checkServerIdentity: () => undefined
}

/** TLS options for full certificate and hostname verification. */
interface PostgresVerifyFullTlsOptions {
  /**
   * PEM-encoded provider root CA certificates. Managed backends supply these;
   * an external database can omit them to use the operating system trust store.
   */
  ca?: string[]
  /** Always true: an untrusted certificate fails the connection. */
  rejectUnauthorized: true
  /** `verify-full` must never override Node's hostname verification. */
  checkServerIdentity?: never
}

/** Verified TLS options ready to pass to node-postgres. */
export type PostgresTlsOptions = PostgresVerifyCaTlsOptions | PostgresVerifyFullTlsOptions

/** Connection fields shared by every Postgres TLS mode. */
interface PostgresConnectionFields {
  /**
   * `postgres://user:password@host:port/database?sslmode=<mode>`. The username,
   * password, and database are percent-encoded to the RFC 3986 unreserved set, so a
   * generated password containing URL-special characters can never corrupt the URL.
   */
  connectionString: string
  /** Address to dial — the cluster writer endpoint for Aurora, the host elsewhere. */
  host: string
  /** TCP port. */
  port: number
  /** Database name. */
  database: string
  /** Role to connect as. */
  username: string
  /**
   * Connection password. For the managed cloud backends this was read from the
   * cloud secret store when the binding resolved; the binding itself only ever
   * carries a locator for it.
   */
  password: string
}

interface PostgresDisableConnection extends PostgresConnectionFields {
  sslmode: "disable"
  ssl: false
}

interface PostgresVerifyCaConnection extends PostgresConnectionFields {
  sslmode: "verify-ca"
  ssl: PostgresVerifyCaTlsOptions
}

interface PostgresVerifyFullConnection extends PostgresConnectionFields {
  sslmode: "verify-full"
  ssl: PostgresVerifyFullTlsOptions
}

/**
 * Everything a Postgres driver needs to connect, discriminated by {@link sslmode}.
 *
 * For node-postgres, pass the individual fields with `ssl` rather than combining
 * `connectionString` and `ssl`. node-postgres parses URL TLS parameters and can
 * overwrite an explicit `ssl` object, including provider roots.
 *
 * External (BYO) bindings default to `verify-full`. A plaintext-only legacy server
 * must be configured explicitly with `sslMode: "disable"` in its external binding.
 */
export type PostgresConnection =
  | PostgresDisableConnection
  | PostgresVerifyCaConnection
  | PostgresVerifyFullConnection

/**
 * A resolved Postgres binding.
 *
 * Unlike the other kinds this exposes no operations: every Postgres backend speaks
 * the same wire protocol, so the binding hands back connection details and the
 * application connects with its own driver.
 */
export interface Postgres {
  /**
   * Resolve the connection details.
   *
   * For a managed cloud backend the first call reads the password from that cloud's
   * secret store with the workload's own identity; the resolved value is then reused,
   * so call the factory again to pick up a rotated password.
   */
  connection(): Promise<PostgresConnection>
}

/** A resolved vault (secrets) binding. */
export interface Vault {
  /** Get the secret named `name` as a string. */
  get(name: string): Promise<string>
  /** Get the secret named `name`, parsed as JSON. */
  getJson<T = unknown>(name: string): Promise<T>
  /** Create or update the secret named `name` with a string value. */
  put(name: string, value: string): Promise<void>
  /** Create or update the secret named `name`, serialized as JSON. */
  putJson(name: string, value: unknown): Promise<void>
  /** Delete the secret named `name`. */
  delete(name: string): Promise<void>
  /** List the names of all secrets in this vault. */
  list(): Promise<string[]>
}

/** A live sandbox session. */
export interface SandboxSession {
  /** Session id, which is what every later call addresses. */
  sessionId: string
  /** Lifecycle state. */
  state: "starting" | "running" | "suspended" | "terminated"
  /** Increments when a session is replaced, so a stale handle is detectable. */
  generation: number
}

/** One frame of a running command's output. */
export type CommandFrame =
  | { kind: "stdout" | "stderr"; seq: number; data: Buffer }
  | { kind: "exit"; exitCode: number; truncated: boolean }

/** What a command needs to run. */
export interface RunCommandOptions {
  /**
   * How long the command may run, in milliseconds. Required rather than defaulted: a defaulted
   * deadline is a hang waiting for a slow day, in a sandbox running code you do not control.
   *
   * It bounds the command, not the call. What expiry does to the session differs by backend:
   * where the backend has no timeout of its own the only lever is ending the session, so the
   * iterator raises once that is confirmed, somewhat after the deadline. Where the agent
   * supervises the process it kills the process group and the session stays usable. Either way
   * the command is stopped; only the session's fate differs.
   */
  deadlineMs: number
  /** Working directory inside the sandbox. */
  workingDirectory?: string
  /** Environment for this command, on top of whatever the session was created with. */
  env?: Record<string, string>
}

/**
 * An isolated environment for running untrusted code.
 *
 * Capabilities differ per platform. Call `capabilities()` and branch, or call and handle the
 * error: an unsupported operation raises rather than silently doing nothing.
 */
export interface Sandbox {
  /** Which operations this platform supports. */
  capabilities(): Promise<string[]>
  /** Creates a session. */
  create(options?: {
    sessionId?: string
    tenantKey?: string
    /** Environment every command in the session starts with. */
    env?: Record<string, string>
  }): Promise<SandboxSession>
  /** Fetches a session, or `null` if it does not exist. Requires `reconnect`. */
  get(sessionId: string): Promise<SandboxSession | null>
  /** Fetches a session, creating it if absent. */
  getOrCreate(options?: {
    sessionId?: string
    tenantKey?: string
    /** Environment every command in the session starts with. */
    env?: Record<string, string>
  }): Promise<SandboxSession>
  /**
   * Lists this sandbox's sessions. Not offered on AWS, Azure or GCP — those raise rather than
   * enumerate. Reach a session whose id you hold with `get`.
   */
  list(): Promise<SandboxSession[]>
  /**
   * Runs a command, yielding frames as the command produces them.
   *
   * The iterator is pull-based all the way down: nothing is read from the sandbox until the
   * loop asks for the next frame, so a slow consumer slows the producer instead of buffering.
   */
  runCommand(
    sessionId: string,
    command: string[],
    options: RunCommandOptions,
  ): AsyncIterable<CommandFrame>
  /** Reads a file out of the sandbox. Requires `files`. */
  readFile(sessionId: string, path: string): Promise<Buffer>
  /** Writes files into the sandbox. Requires `files`. */
  writeFiles(sessionId: string, files: Record<string, Buffer | string>): Promise<void>
  /** Creates a directory inside the sandbox. Requires `files`. */
  mkdir(sessionId: string, path: string): Promise<void>
  /** Suspends a session, preserving state. Requires `suspendResume`. */
  suspend(sessionId: string): Promise<void>
  /** Resumes a suspended session. Requires `suspendResume`. */
  resume(sessionId: string): Promise<void>
  /** Destroys a session. Idempotent. */
  terminate(sessionId: string): Promise<void>
}
