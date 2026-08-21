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

import {
  AlienError,
  InvalidPostgresTlsConfigError,
  UnknownPostgresSslModeError,
  UnknownSandboxValueError,
  unwrapNapiError,
} from "./errors.js"
import type {
  NativeAddon,
  RawBindingsHandle,
  RawCommandFrame,
  RawCommandStreamHandle,
  RawContainerHandle,
  RawKeyHandle,
  RawKvHandle,
  RawPostgresConnection,
  RawPostgresHandle,
  RawQueueHandle,
  RawRemoteBindingsHandle,
  RawRemoteStorageHandle,
  RawSandboxHandle,
  RawSandboxSession,
  RawStorageHandle,
  RawVaultHandle,
} from "./loader.js"
import type {
  CommandFrame,
  Container,
  Key,
  KeyOptions,
  Kv,
  KvEntry,
  KvScanResult,
  KvSetOptions,
  Postgres,
  PostgresConnection,
  PostgresSslMode,
  PresignedRequest,
  Queue,
  QueueMessage,
  RemoteStorage,
  Sandbox,
  SandboxSession,
  SignedUrlOptions,
  Storage,
  StoragePutOptions,
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
    put: (path, data, options?: StoragePutOptions) =>
      guard(handle, raw => raw.put(path, toBuffer(data), options ?? null)),
    delete: path => guard(handle, raw => raw.delete(path)),
    list: prefix => guard(handle, raw => raw.list(prefix ?? null)),
    head: path => guard(handle, raw => raw.head(path)),
    copy: (from, to) => guard(handle, raw => raw.copy(from, to)),
    signedUrl: (options: SignedUrlOptions): Promise<PresignedRequest> =>
      guard(handle, raw => raw.signedUrl(options.method, options.path, options.expiresIn)),
  }
}

function makeKey(handle: () => Promise<RawKeyHandle>): Key {
  return {
    encrypt: (plaintext, options?: KeyOptions) =>
      guard(handle, raw => raw.encrypt(toBuffer(plaintext), options?.context ?? null)),
    decrypt: (ciphertext, options?: KeyOptions) =>
      guard(handle, raw => raw.decrypt(toBuffer(ciphertext), options?.context ?? null)),
  }
}

function makeRemoteStorage(handle: () => Promise<RawRemoteStorageHandle>): RemoteStorage {
  return {
    get: path => guard(handle, raw => raw.get(path)),
    put: (path, data, options?: StoragePutOptions) =>
      guard(handle, raw => raw.put(path, toBuffer(data), options ?? null)),
    delete: path => guard(handle, raw => raw.delete(path)),
    list: prefix => guard(handle, raw => raw.list(prefix ?? null)),
    head: path => guard(handle, raw => raw.head(path)),
  }
}

/** The session states the addon and this wrapper agree on. */
const SANDBOX_SESSION_STATES = ["starting", "running", "suspended", "terminated"] as const

/** The output frame kinds that carry data; `exit` is handled separately. */
const SANDBOX_STREAM_KINDS = ["stdout", "stderr"] as const

/**
 * Narrows a value the addon produced into a declared union, or throws.
 *
 * Casting would put a value outside the union behind a type claiming otherwise, and a caller's
 * `switch` over the union would then fall through with no error. This is the same version-skew
 * argument `toPostgresConnection` makes for `sslmode`.
 */
function narrow<T extends string>(field: string, value: string, expected: readonly T[]): T {
  if ((expected as readonly string[]).includes(value)) {
    return value as T
  }
  throw new AlienError(
    UnknownSandboxValueError.create({ field, value, expected: [...expected] }).toOptions(),
  )
}

function makeSandbox(handle: () => Promise<RawSandboxHandle>): Sandbox {
  const session = (raw: RawSandboxSession): SandboxSession => ({
    sessionId: raw.sessionId,
    state: narrow("session state", raw.state, SANDBOX_SESSION_STATES),
    generation: raw.generation,
  })

  const frame = (raw: RawCommandFrame): CommandFrame =>
    raw.kind === "exit"
      ? { kind: "exit", exitCode: raw.exitCode ?? -1, truncated: raw.truncated ?? false }
      : {
          kind: narrow("frame kind", raw.kind, SANDBOX_STREAM_KINDS),
          seq: raw.seq ?? 0,
          data: raw.data ?? Buffer.alloc(0),
        }

  return {
    capabilities: () => guard(handle, async raw => raw.capabilities()),
    create: options =>
      guard(handle, async raw =>
        session(
          await raw.create(
            options?.sessionId ?? null,
            options?.tenantKey ?? null,
            options?.env ?? null,
          ),
        ),
      ),
    get: sessionId =>
      guard(handle, async raw => {
        const found = await raw.get(sessionId)
        return found === null ? null : session(found)
      }),
    getOrCreate: options =>
      guard(handle, async raw =>
        session(
          await raw.getOrCreate(
            options?.sessionId ?? null,
            options?.tenantKey ?? null,
            options?.env ?? null,
          ),
        ),
      ),
    list: () => guard(handle, async raw => (await raw.list()).map(session)),
    runCommand: (sessionId, command, options) => ({
      [Symbol.asyncIterator](): AsyncIterator<CommandFrame, undefined> {
        let stream: RawCommandStreamHandle | null = null
        let starting: Promise<RawCommandStreamHandle> | null = null
        let tail: Promise<unknown> = Promise.resolve()
        let finished = false

        // A cancel that lands during setup has no handle yet, so it waits for the one being made:
        // resolving sooner would report a command stopped that is about to start.
        let closing: Promise<void> | null = null
        const close = () => {
          closing ??= (async () => {
            const open = stream ?? (await starting?.catch(() => null)) ?? null
            if (open === null) return
            try {
              await open.close()
            } catch (err) {
              throw unwrapNapiError(err)
            }
          })()
          return closing
        }

        // Cancellation bypasses the pull queue so it can release a native read waiting for output.
        const finish = async () => {
          finished = true
          await close()
        }

        // The fault iteration ended on is what the caller must see; a close that also fails is
        // reported only when it is the sole failure, from `return()`.
        const fail = async (err: unknown): Promise<never> => {
          await finish().catch(() => undefined)
          throw err
        }

        // A pull that observes a cancel only reports `done`: the `return()` or `throw()` that
        // cancelled is where a failed close is reported, and it must not be reported twice.
        const cancelled = async (): Promise<IteratorResult<CommandFrame, undefined>> => {
          await finish().catch(() => undefined)
          return { done: true, value: undefined }
        }

        return {
          next() {
            const pull = tail.then(async (): Promise<IteratorResult<CommandFrame, undefined>> => {
              if (finished) {
                return { done: true, value: undefined }
              }
              let open = stream
              if (open === null) {
                starting = guard(handle, raw =>
                  raw.runCommand(
                    sessionId,
                    command,
                    options.deadlineMs,
                    options.workingDirectory ?? null,
                    options.env ?? null,
                  ),
                )
                try {
                  open = await starting
                  stream = open
                } catch (err) {
                  finished = true
                  throw err
                }
              }

              // A return during setup cannot close a handle until this pull receives it.
              if (finished) {
                return cancelled()
              }

              let next: RawCommandFrame | null
              try {
                next = await open.next()
              } catch (err) {
                return fail(unwrapNapiError(err))
              }

              if (finished) {
                return cancelled()
              }
              if (next === null) {
                await finish()
                return { done: true, value: undefined }
              }

              let value: CommandFrame
              try {
                value = frame(next)
              } catch (err) {
                return fail(unwrapNapiError(err))
              }
              return { done: false, value }
            })

            // A rejected pull must not poison the queue behind it.
            tail = pull.catch(() => undefined)
            return pull
          },
          async return() {
            await finish()
            return { done: true, value: undefined }
          },
          throw: (err: unknown) => fail(err),
        }
      },
    }),
    readFile: (sessionId, path) => guard(handle, raw => raw.readFile(sessionId, path)),
    writeFiles: (sessionId, files) =>
      guard(handle, async raw => {
        for (const [path, contents] of Object.entries(files)) {
          await raw.writeFile(
            sessionId,
            path,
            typeof contents === "string" ? Buffer.from(contents, "utf8") : contents,
          )
        }
      }),
    mkdir: (sessionId, path) => guard(handle, raw => raw.mkdir(sessionId, path)),
    suspend: sessionId => guard(handle, raw => raw.suspend(sessionId)),
    resume: sessionId => guard(handle, raw => raw.resume(sessionId)),
    terminate: sessionId => guard(handle, raw => raw.terminate(sessionId)),
  }
}

function makeKv(handle: () => Promise<RawKvHandle>): Kv {
  return {
    get: key => guard(handle, raw => raw.get(key)),
    getText: key =>
      guard(handle, async raw => {
        const entry = await raw.get(key)
        return entry === null ? null : { ...entry, value: entry.value.toString("utf8") }
      }),
    getJson: <T = unknown>(key: string): Promise<KvEntry<T> | null> =>
      guard(handle, async raw => {
        const entry = await raw.get(key)
        return entry === null
          ? null
          : { ...entry, value: JSON.parse(entry.value.toString("utf8")) as T }
      }),
    set: (key, value, options?: KvSetOptions) =>
      guard(handle, raw =>
        raw.put(
          key,
          Buffer.from(value, "utf8"),
          options?.ttl ?? null,
          options && "ifVersion" in options
            ? options.ifVersion === null
              ? "absent"
              : "version"
            : null,
          typeof options?.ifVersion === "string" ? options.ifVersion : null,
        ),
      ),
    setJson: (key, value, options?: KvSetOptions) =>
      guard(handle, raw =>
        raw.put(
          key,
          Buffer.from(JSON.stringify(value), "utf8"),
          options?.ttl ?? null,
          options && "ifVersion" in options
            ? options.ifVersion === null
              ? "absent"
              : "version"
            : null,
          typeof options?.ifVersion === "string" ? options.ifVersion : null,
        ),
      ),
    delete: (key, options) => guard(handle, raw => raw.delete(key, options?.ifVersion ?? null)),
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

const POSTGRES_SSL_MODES = {
  disable: true,
  "verify-ca": true,
  "verify-full": true,
} satisfies Record<PostgresSslMode, true>

function postgresSslModeLabel(sslmode: unknown): string {
  return typeof sslmode === "string" ? sslmode : String(sslmode)
}

function invalidPostgresTlsConfig(sslmode: unknown, reason: string): AlienError {
  return new AlienError(
    InvalidPostgresTlsConfigError.create({
      sslmode: postgresSslModeLabel(sslmode),
      reason,
    }).toOptions(),
  )
}

function isPostgresSslMode(value: unknown): value is PostgresSslMode {
  return typeof value === "string" && Object.hasOwn(POSTGRES_SSL_MODES, value)
}

function hasAtLeastOne<T>(values: T[]): values is [T, ...T[]] {
  return values.length > 0
}

/**
 * Decode the raw addon connection into the public discriminated union, deriving
 * node-postgres TLS options from the Rust-resolved mode and provider roots.
 *
 * An unrecognized `sslmode` means the addon and this wrapper disagree about the
 * `SslMode` enum — a version skew that must fail loudly rather than produce a
 * connection with a silently wrong TLS posture. It throws
 * {@link UnknownPostgresSslModeError} so a caller can discriminate it by `code`;
 * `guard` passes an `AlienError` through untouched.
 */
function toPostgresConnection(raw: RawPostgresConnection): PostgresConnection {
  if (!isPostgresSslMode(raw.sslmode)) {
    throw new AlienError(
      UnknownPostgresSslModeError.create({
        sslmode: postgresSslModeLabel(raw.sslmode),
        expected: Object.keys(POSTGRES_SSL_MODES),
      }).toOptions(),
    )
  }

  if (!Array.isArray(raw.caCertificates)) {
    throw invalidPostgresTlsConfig(raw.sslmode, "caCertificates must be an array")
  }

  const caCertificates: string[] = []
  const rawCaCertificates: unknown[] = raw.caCertificates
  for (const [index, certificate] of rawCaCertificates.entries()) {
    if (typeof certificate !== "string") {
      throw invalidPostgresTlsConfig(raw.sslmode, `caCertificates[${index}] must be a string`)
    }
    if (certificate.trim().length === 0) {
      throw invalidPostgresTlsConfig(raw.sslmode, `caCertificates[${index}] cannot be empty`)
    }
    caCertificates.push(certificate)
  }

  const fields = {
    connectionString: raw.connectionString,
    host: raw.host,
    port: raw.port,
    database: raw.database,
    username: raw.username,
    password: raw.password,
  }

  switch (raw.sslmode) {
    case "disable":
      if (caCertificates.length > 0) {
        throw invalidPostgresTlsConfig(raw.sslmode, "disable cannot carry CA certificates")
      }
      return { ...fields, sslmode: raw.sslmode, ssl: false }
    case "verify-ca":
      if (!hasAtLeastOne(caCertificates)) {
        throw invalidPostgresTlsConfig(
          raw.sslmode,
          "verify-ca requires at least one CA certificate",
        )
      }
      return {
        ...fields,
        sslmode: raw.sslmode,
        ssl: {
          ca: caCertificates,
          rejectUnauthorized: true,
          // Node calls this only after the CA chain succeeds. Cloud SQL's
          // per-instance CA authenticates the server even though its certificate
          // cannot match the PSC consumer endpoint IP.
          checkServerIdentity: (): undefined => undefined,
        },
      }
    case "verify-full":
      return {
        ...fields,
        sslmode: raw.sslmode,
        ssl: {
          ...(caCertificates.length > 0 ? { ca: caCertificates } : {}),
          rejectUnauthorized: true,
        },
      }
    default: {
      const unhandledSslMode: never = raw.sslmode
      return unhandledSslMode
    }
  }
}

function makePostgres(handle: () => Promise<RawPostgresHandle>): Postgres {
  return {
    connection: (): Promise<PostgresConnection> =>
      guard(handle, async raw => toPostgresConnection(raw.connection())),
  }
}

/** The public factory surface. */
export interface Factories {
  storage(name: string): Storage
  key(name: string): Key
  kv(name: string): Kv
  queue(name: string): Queue
  vault(name: string): Vault
  container(name: string): Container
  postgres(name: string): Postgres
  sandbox(name: string): Sandbox
}

/** Build the factories bound to a given addon provider. */
export function createFactories(getAddon: () => NativeAddon): Factories {
  const getBindings = bindingsFromAddon(getAddon)
  return {
    storage: name => makeStorage(lazyHandle(async () => (await getBindings()).storage(name))),
    key: name => makeKey(lazyHandle(async () => (await getBindings()).key(name))),
    kv: name => makeKv(lazyHandle(async () => (await getBindings()).kv(name))),
    queue: name => makeQueue(lazyHandle(async () => (await getBindings()).queue(name))),
    vault: name => makeVault(lazyHandle(async () => (await getBindings()).vault(name))),
    container: name => makeContainer(lazyHandle(async () => (await getBindings()).container(name))),
    postgres: name => makePostgres(lazyHandle(async () => (await getBindings()).postgres(name))),
    sandbox: name => makeSandbox(lazyHandle(async () => (await getBindings()).sandbox(name))),
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

/** Build the remote Key factory around one native bindings handle. */
export function createRemoteKeyFactory(bindings: RawRemoteBindingsHandle) {
  const keys = new Map<string, Key>()
  return (name: string): Key => {
    let key = keys.get(name)
    if (!key) {
      key = makeKey(lazyHandle(() => bindings.key(name)))
      keys.set(name, key)
    }
    return key
  }
}
