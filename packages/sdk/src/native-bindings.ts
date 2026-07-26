/**
 * `@alienplatform/sdk/native-bindings`: the embedded-bindings bridge for bundled
 * Workers.
 *
 * Same one-hop trick as `./native` — a Worker can resolve the SDK but not the
 * transitive `@alienplatform/bindings` — minus the gateway. A bundled Worker gets
 * its launcher from the versioned base image at `./alien-ai-gateway`, so importing
 * the gateway's `/native` here would both register a `.bin` asset path that no
 * bundle packages and make the build fail wherever that asset was never staged.
 */

import { installEmbeddedAddon as installBindingsAddon } from "@alienplatform/bindings/native"

/** Register the bun-embedded bindings addon with its loader. */
export function installEmbeddedAddon(): void {
  installBindingsAddon()
}
