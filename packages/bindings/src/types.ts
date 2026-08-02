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

/** Storage operations available from an external deployment binding. */
export type RemoteStorage = Pick<Storage, "get" | "put" | "delete" | "list" | "head">

/** Options for {@link Kv.set}. */
export interface KvSetOptions {
  /** Time-to-live, in seconds. */
  ttl?: number
  /** Only create the key if it does not already exist. */
  ifNotExists?: boolean
}

/** A single key-value pair returned by a scan. */
export interface KvScanItem {
  /** The key. */
  key: string
  /** The raw value bytes. */
  value: Buffer
}

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
  /** Get the raw value bytes for `key`, or `null` if absent/expired. */
  get(key: string): Promise<Buffer | null>
  /** Get the value for `key` as UTF-8 text, or `null` if absent/expired. */
  getText(key: string): Promise<string | null>
  /** Get the value for `key` parsed as JSON, or `null` if absent/expired. */
  getJson<T = unknown>(key: string): Promise<T | null>
  /**
   * Set `key` to the UTF-8 `value`. With `ifNotExists`, resolves `true` when
   * created and `false` when the key already existed; otherwise `true`.
   */
  set(key: string, value: string, options?: KvSetOptions): Promise<boolean>
  /**
   * Set `key` to `value` serialized as JSON (via `JSON.stringify`). With
   * `ifNotExists`, resolves `true` when created and `false` when the key already
   * existed; otherwise `true`.
   */
  setJson(key: string, value: unknown, options?: KvSetOptions): Promise<boolean>
  /** Delete `key` (no error if absent). */
  delete(key: string): Promise<void>
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
