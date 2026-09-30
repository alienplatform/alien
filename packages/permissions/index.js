import { bytes } from "./generated/bytes.js"
import { initSync, operation_permissions_json } from "./generated/compiler.js"

let initialized = false

/** Execute a JSON request with the same Rust implementation used by the CLI. */
export function executeOperationPermissions(request) {
  if (!initialized) {
    initSync({ module: Uint8Array.from(atob(bytes), character => character.charCodeAt(0)) })
    initialized = true
  }
  return operation_permissions_json(request)
}
