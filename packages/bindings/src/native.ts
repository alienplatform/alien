/**
 * `@alienplatform/bindings/native` — static-embed entry for `bun build --compile`.
 *
 * This module imports the native addon through the literal specifier
 * `./alien-bindings.node` so bun's compiler can detect the reference and embed
 * the addon into the single-file executable. Unlike the default entry, it does
 * NOT probe the filesystem or resolve a prebuild package — the addon must
 * already be staged next to the built `native.js`.
 *
 * Staging contract (produced by `alien build`, task 13): before this module is
 * bundled and compiled, the correct per-platform addon is copied next to
 * `dist/native.js` as `alien-bindings.node`. Task 13 owns that copy step; this
 * module only consumes the staged file.
 *
 * The specifier is kept external at build time (see tsdown.config.ts) so the
 * literal survives into `dist/native.js` for bun to resolve and embed.
 */

import addon from "./alien-bindings.node"
import { createFactories } from "./factories.js"

const factories = createFactories(() => addon)

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
