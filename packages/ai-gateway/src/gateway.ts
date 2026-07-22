/**
 * The gateway surface, parameterized over how the native addon is obtained, so the
 * lazy-loading entry (`index.ts`) and the static-embed entry (`native.ts`) share
 * one implementation — mirroring `@alienplatform/bindings`' `createFactories`.
 */

import type { NativeAddon, RawAiGatewayHandle } from "./loader.js"
import { unwrapNapiError } from "./napi-error.js"

export interface Gateway {
  startAiGateway(): Promise<RawAiGatewayHandle>
}

export function createGateway(getAddon: () => NativeAddon): Gateway {
  // Started once per process: the handle is held for the process lifetime, since dropping it
  // aborts the Rust server. Only a *resolved* start is memoized — caching a rejection would
  // turn one transient credential failure into a permanently dead gateway, even though the
  // Rust side marks those errors retryable.
  let started: Promise<RawAiGatewayHandle> | null = null

  // `async` so a caller's `.catch()` sees a rejection: loading the addon throws synchronously.
  async function startAiGateway(): Promise<RawAiGatewayHandle> {
    started ??= (async () => getAddon().startAiGateway())().catch(error => {
      started = null
      throw unwrapNapiError(error)
    })
    return started
  }

  return { startAiGateway }
}
