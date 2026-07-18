// EXAMPLE: Overlapping command names, routed by target.
//
// Two command-capable resources live in one deployment and BOTH register a
// command called `status`:
//
//   - Worker  "api"            — push model: the SDK `command()` registrar.
//   - Daemon  "indexer-daemon" — pull model: the `@alienplatform/commands`
//                                receiver, which leases commands over outbound
//                                HTTPS and also reads a `kv` binding directly.
//
// Because the names collide, a sender MUST say which resource it means. The
// sender (services/sender) invokes `status` with `.target("api")` and with
// `.target("indexer-daemon")` and gets two different answers — proof that the
// command server routes by target resource id, not by command name alone.

import * as alien from "@alienplatform/core"

// Shared index the daemon maintains and both resources report on.
const index = new alien.Kv("index").build()

// Worker: HTTP + the `status`/`search` commands via the SDK push registrar.
const api = new alien.Worker("api")
  .code({ type: "source", src: "./services/api", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .publicEndpoint("api")
  .link(index)
  .permissions("execution")
  .build()

// Daemon: a resident process that leases the SAME command names through the
// pull receiver and serves them from its own view of the index.
const indexer = new alien.Daemon("indexer-daemon")
  .code({ type: "source", src: "./services/indexer", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .link(index)
  .permissions("execution")
  .build()

export default new alien.Stack("command-routing")
  .platforms(["local", "aws", "gcp", "azure", "kubernetes"])
  .add(index, "frozen")
  .add(api, "live")
  .add(indexer, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["kv/data-read", "kv/data-write"],
      },
    },
  })
  .build()
