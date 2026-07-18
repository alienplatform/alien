/**
 * The four binding factories, parameterized over how the native addon is
 * obtained. `index.ts` binds them to the lazy {@link loadAddon}; `native.ts`
 * binds them to a statically-embedded addon.
 *
 * Laziness contract: constructing a factory result performs no work. The first
 * operation on a handle materializes the `BindingsHandle` and the resource
 * handle exactly once, caches both, and every operation translates thrown napi
 * errors through {@link unwrapNapiError}. A failed materialization is not
 * cached, so a later call retries.
 *
 * Handle methods are closures (not `this`-bound class methods), so they behave
 * correctly even when destructured off the handle (`const { get } = storage(x)`).
 */

import { unwrapNapiError } from "./errors.js"
import type {
  NativeAddon,
  RawBindingsHandle,
  RawKvHandle,
  RawQueueHandle,
  RawStorageHandle,
  RawVaultHandle,
} from "./loader.js"
import type {
  Kv,
  KvScanResult,
  KvSetOptions,
  PresignedRequest,
  Queue,
  QueueMessage,
  SignedUrlOptions,
  Storage,
  Vault,
} from "./types.js"

/**
 * Build a lazy, cached resolver for one resource handle. The returned function
 * loads the addon, constructs a `BindingsHandle`, and resolves the resource
 * handle on first call; subsequent calls reuse the cached handle.
 */
function lazyHandle<THandle>(
  getAddon: () => NativeAddon,
  name: string,
  resolve: (bindings: RawBindingsHandle, name: string) => Promise<THandle>,
): () => Promise<THandle> {
  let pending: Promise<THandle> | undefined

  return () => {
    if (!pending) {
      pending = (async () => {
        const addon = getAddon()
        const bindings = new addon.BindingsHandle()
        return await resolve(bindings, name)
      })().catch(err => {
        // Do not cache a failed materialization; allow a later retry.
        pending = undefined
        throw err
      })
    }
    return pending
  }
}

function toBuffer(data: Buffer | Uint8Array): Buffer {
  return Buffer.isBuffer(data) ? data : Buffer.from(data)
}

/** Run `op` against the resolved handle, translating any napi error. */
async function guard<THandle, TResult>(
  handle: () => Promise<THandle>,
  op: (raw: THandle) => Promise<TResult>,
): Promise<TResult> {
  try {
    return await op(await handle())
  } catch (err) {
    throw unwrapNapiError(err)
  }
}

function makeStorage(handle: () => Promise<RawStorageHandle>): Storage {
  return {
    get: path => guard(handle, raw => raw.get(path)),
    put: (path, data) => guard(handle, raw => raw.put(path, toBuffer(data))),
    delete: path => guard(handle, raw => raw.delete(path)),
    list: prefix => guard(handle, raw => raw.list(prefix ?? null)),
    head: path => guard(handle, raw => raw.head(path)),
    copy: (from, to) => guard(handle, raw => raw.copy(from, to)),
    signedUrl: (options: SignedUrlOptions): Promise<PresignedRequest> =>
      guard(handle, raw => raw.signedUrl(options.method, options.path, options.expiresIn)),
  }
}

function makeKv(handle: () => Promise<RawKvHandle>): Kv {
  return {
    get: key => guard(handle, raw => raw.get(key)),
    getText: key =>
      guard(handle, async raw => {
        const value = await raw.get(key)
        return value === null ? null : value.toString("utf8")
      }),
    getJson: <T = unknown>(key: string): Promise<T | null> =>
      guard(handle, async raw => {
        const value = await raw.get(key)
        return value === null ? null : (JSON.parse(value.toString("utf8")) as T)
      }),
    set: (key, value, options?: KvSetOptions) =>
      guard(handle, raw =>
        raw.put(
          key,
          Buffer.from(value, "utf8"),
          options?.ttl ?? null,
          options?.ifNotExists ?? null,
        ),
      ),
    setJson: (key, value, options?: KvSetOptions) =>
      guard(handle, raw =>
        raw.put(
          key,
          Buffer.from(JSON.stringify(value), "utf8"),
          options?.ttl ?? null,
          options?.ifNotExists ?? null,
        ),
      ),
    delete: key => guard(handle, raw => raw.delete(key)),
    exists: key => guard(handle, raw => raw.exists(key)),
    // The napi scan already returns each key with its value bytes; pass them
    // straight through rather than dropping the values.
    scan: (prefix, limit, cursor): Promise<KvScanResult> =>
      guard(handle, async raw => {
        const result = await raw.scan(prefix, limit ?? null, cursor ?? null)
        return { items: result.items, nextCursor: result.nextCursor }
      }),
  }
}

// The napi queue methods take the queue name as their first argument; the
// binding name is used for it (providers key the queue off the binding).
function makeQueue(handle: () => Promise<RawQueueHandle>, name: string): Queue {
  return {
    send: message => guard(handle, raw => raw.sendJson(name, JSON.stringify(message))),
    sendText: text => guard(handle, raw => raw.sendText(name, text)),
    receive: (max): Promise<QueueMessage[]> => guard(handle, raw => raw.receive(name, max)),
    ack: receipt => guard(handle, raw => raw.ack(name, receipt)),
    nack: receipt => guard(handle, raw => raw.nack(name, receipt)),
    purge: () => guard(handle, raw => raw.purge(name)),
  }
}

function makeVault(handle: () => Promise<RawVaultHandle>): Vault {
  return {
    get: name => guard(handle, raw => raw.getSecret(name)),
    getJson: <T = unknown>(name: string): Promise<T> =>
      guard(handle, async raw => JSON.parse(await raw.getSecret(name)) as T),
    put: (name, value) => guard(handle, raw => raw.setSecret(name, value)),
    putJson: (name, value) => guard(handle, raw => raw.setSecret(name, JSON.stringify(value))),
    delete: name => guard(handle, raw => raw.deleteSecret(name)),
    list: (): Promise<string[]> => guard(handle, raw => raw.listSecrets()),
  }
}

/** The public factory surface. */
export interface Factories {
  storage(name: string): Storage
  kv(name: string): Kv
  queue(name: string): Queue
  vault(name: string): Vault
}

/** Build the four factories bound to a given addon provider. */
export function createFactories(getAddon: () => NativeAddon): Factories {
  return {
    storage: name => makeStorage(lazyHandle(getAddon, name, (b, n) => b.storage(n))),
    kv: name => makeKv(lazyHandle(getAddon, name, (b, n) => b.kv(n))),
    queue: name =>
      makeQueue(
        lazyHandle(getAddon, name, (b, n) => b.queue(n)),
        name,
      ),
    vault: name => makeVault(lazyHandle(getAddon, name, (b, n) => b.vault(n))),
  }
}
