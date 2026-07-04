/**
 * Static-embed entry point for the `bun build --compile` check in `run.ts`.
 *
 * It imports `@alienplatform/bindings` through the pinned `./native` subpath —
 * the statically analyzable specifier that lets the Bun compiler stage the
 * platform `.node` addon into a single-file binary (bindings PACKAGE_LAYOUT.md,
 * "Exports map"). Unlike `imports.ts`, this uses a STATIC import on purpose:
 * `bun build --compile` only follows statically analyzable imports.
 *
 * Until task 04 ships `@alienplatform/bindings` and task 04a ships the
 * per-platform `.node` prebuilds, this specifier does not resolve, so both the
 * fixture typecheck and the `bun build --compile` step fail — `run.ts` marks
 * those `[expected]`.
 */

import { storage } from "@alienplatform/bindings/native"

// Reference the import so the compiler must stage the native addon, then exit 0
// so a successfully compiled binary is observably runnable.
const factory: unknown = storage
if (typeof factory !== "function") {
  throw new Error("expected the ./native storage factory to be a function")
}

console.log("compile-entry: native binding factory embedded")
