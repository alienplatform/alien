/**
 * `@alienplatform/bindings` — direct TypeScript bindings for Alien storage, kv,
 * queue, and vault, backed by an in-process napi-rs addon over the Rust
 * `alien-bindings` crate.
 *
 * Constructing a factory (`storage("x")`, `kv("y")`, …) does no I/O and needs no
 * addon; the native module loads on the first operation. The first operation
 * against a binding with no `ALIEN_<NAME>_BINDING` in the environment throws
 * {@link BindingNotConfiguredError}.
 */

import { createFactories } from "./factories.js"
import { loadAddon } from "./loader.js"

const factories = createFactories(loadAddon)

/** Resolve the storage binding named `name`. */
export const storage = factories.storage
/** Resolve the key-value binding named `name`. */
export const kv = factories.kv
/** Resolve the queue binding named `name`. */
export const queue = factories.queue
/** Resolve the vault binding named `name`. */
export const vault = factories.vault

export {
  AlienError,
  BindingNotConfiguredError,
  defineError,
  unwrapNapiError,
} from "./errors.js"

export type {
  BindingOptions,
  Kv,
  KvScanResult,
  KvSetOptions,
  ObjectMeta,
  PresignedRequest,
  Queue,
  QueueMessage,
  SignedUrlMethod,
  SignedUrlOptions,
  Storage,
  Vault,
} from "./types.js"
