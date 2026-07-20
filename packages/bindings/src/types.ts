/**
 * Public types for `@alienplatform/bindings`: the four resource handle
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

/** A resolved object-storage binding. */
export interface Storage {
  /** Fetch the object at `path`. */
  get(path: string): Promise<Buffer>
  /** Store `data` at `path`. */
  put(path: string, data: Buffer | Uint8Array): Promise<void>
  /** Delete the object at `path`. */
  delete(path: string): Promise<void>
  /** List objects, optionally filtered by `prefix`. */
  list(prefix?: string): Promise<ObjectMeta[]>
  /** Fetch metadata for the object at `path`. */
  head(path: string): Promise<ObjectMeta>
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
