/**
 * The binding factories, parameterized over how the native addon is
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
  RawContainerHandle,
  RawKvHandle,
  RawQueueHandle,
  RawRemoteBindingsHandle,
  RawRemoteStorageHandle,
  RawStorageHandle,
  RawVaultHandle,
} from "./loader.js"
import type {
  Container,
  Kv,
  KvScanResult,
  KvSetOptions,
  PresignedRequest,
  Queue,
  QueueMessage,
  RemoteStorage,
  SignedUrlOptions,
  Storage,
  Vault,
} from "./types.js"

type BindingsHandleProvider = () => Promise<RawBindingsHandle>

/**
 * Build a lazy, cached resolver for one resource handle. The returned function
 * obtains a `BindingsHandle` and resolves the resource handle on first call;
 * subsequent calls reuse the cached handle.
 */
function lazyHandle<THandle>(resolve: () => Promise<THandle>): () => Promise<THandle> {
  let pending: Promise<THandle> | undefined

  return () => {
    if (!pending) {
      pending = resolve().catch(err => {
        // Do not cache a failed materialization; allow a later retry.
        pending = undefined
        throw err
      })
    }
    return pending
  }
}

function bindingsFromAddon(getAddon: () => NativeAddon): BindingsHandleProvider {
  return async () => {
    const addon = getAddon()
    return new addon.BindingsHandle()
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

function makeRemoteStorage(handle: () => Promise<RawRemoteStorageHandle>): RemoteStorage {
  return {
    get: path => guard(handle, raw => raw.get(path)),
    put: (path, data) => guard(handle, raw => raw.put(path, toBuffer(data))),
    delete: path => guard(handle, raw => raw.delete(path)),
    list: prefix => guard(handle, raw => raw.list(prefix ?? null)),
    head: path => guard(handle, raw => raw.head(path)),
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

// The native bound queue already carries its configured queue name.
function makeQueue(handle: () => Promise<RawQueueHandle>): Queue {
  return {
    send: message => guard(handle, raw => raw.sendJson(JSON.stringify(message))),
    sendText: text => guard(handle, raw => raw.sendText(text)),
    receive: (max): Promise<QueueMessage[]> => guard(handle, raw => raw.receive(max)),
    ack: receipt => guard(handle, raw => raw.ack(receipt)),
    nack: receipt => guard(handle, raw => raw.nack(receipt)),
    purge: () => guard(handle, raw => raw.purge()),
  }
}

function makeContainer(handle: () => Promise<RawContainerHandle>): Container {
  return {
    getInternalUrl: () => guard(handle, raw => raw.getInternalUrl()),
    getPublicUrl: () => guard(handle, raw => raw.getPublicUrl()),
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
  container(name: string): Container
}

/** Build the factories bound to a given addon provider. */
export function createFactories(getAddon: () => NativeAddon): Factories {
  const getBindings = bindingsFromAddon(getAddon)
  return {
    storage: name => makeStorage(lazyHandle(async () => (await getBindings()).storage(name))),
    kv: name => makeKv(lazyHandle(async () => (await getBindings()).kv(name))),
    queue: name => makeQueue(lazyHandle(async () => (await getBindings()).queue(name))),
    vault: name => makeVault(lazyHandle(async () => (await getBindings()).vault(name))),
    container: name => makeContainer(lazyHandle(async () => (await getBindings()).container(name))),
  }
}

/** Build the remote-only storage factory around one native bindings handle. */
export function createRemoteStorageFactory(bindings: RawRemoteBindingsHandle) {
  const storages = new Map<string, RemoteStorage>()
  return (name: string): RemoteStorage => {
    let storage = storages.get(name)
    if (!storage) {
      storage = makeRemoteStorage(lazyHandle(() => bindings.storage(name)))
      storages.set(name, storage)
    }
    return storage
  }
}
