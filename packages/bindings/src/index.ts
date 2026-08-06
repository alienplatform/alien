/**
 * `@alienplatform/bindings` — direct TypeScript bindings for Alien storage, kv,
 * queue, vault, linked containers, and Postgres, backed by an in-process napi-rs addon
 * over the Rust `alien-bindings` crate.
 *
 * Constructing a factory (`storage("x")`, `kv("y")`, …) does no I/O and needs no
 * addon; the native module loads on the first operation. The first operation
 * against a binding with no `ALIEN_<NAME>_BINDING` in the environment throws
 * {@link BindingNotConfiguredError}.
 */

import { createFactories } from "./factories.js"
import { loadAddon } from "./loader.js"

export type {
  RemoteAiBinding,
  RemoteAiClientConfig,
  RemoteAiLease,
  RemoteDeploymentBindingsOptions,
} from "./remote.js"
export { Bindings } from "./remote.js"

const factories = createFactories(loadAddon)

/** Resolve the storage binding named `name`. */
export const storage = factories.storage
/** Resolve the provider-backed key binding named `name`. */
export const key = factories.key
/** Resolve the key-value binding named `name`. */
export const kv = factories.kv
/** Resolve the queue binding named `name`. */
export const queue = factories.queue
/** Resolve the vault binding named `name`. */
export const vault = factories.vault
/** Resolve the linked-container binding named `name`. */
export const container = factories.container
/** Resolve the Postgres binding named `name`. */
export const postgres = factories.postgres

export {
  AlienError,
  BindingNotConfiguredError,
  BindingNotFoundError,
  defineError,
  InvalidPostgresTlsConfigError,
  UnknownPostgresSslModeError,
} from "./errors.js"

export type {
  Container,
  Key,
  KeyOptions,
  Kv,
  KvScanItem,
  KvScanResult,
  KvSetOptions,
  ObjectMeta,
  Postgres,
  PostgresConnection,
  PostgresSslMode,
  PostgresTlsOptions,
  PresignedRequest,
  Queue,
  QueueMessage,
  RemoteStorage,
  SignedUrlMethod,
  SignedUrlOptions,
  Storage,
  StorageGetResult,
  StorageHeadResult,
  StorageObjectAttributes,
  StoragePutAttributes,
  StoragePutOptions,
  StoragePutResult,
  Vault,
} from "./types.js"
