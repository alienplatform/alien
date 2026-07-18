/**
 * Minimal read-only npm registry for the addon release smoke.
 *
 * The smoke packs the exact core, wrapper, and platform artifacts that would be
 * published, then serves their package metadata and tarballs locally. This
 * lets npm and Bun resolve the wrapper's normal semver dependencies and
 * optionalDependencies without making the consumer list those packages
 * directly. It is intentionally not a general-purpose registry.
 */

import { createHash } from "node:crypto"
import { readFileSync, writeFileSync } from "node:fs"
import { createServer } from "node:http"

const [configPath, readyPath] = process.argv.slice(2)
if (!configPath || !readyPath) {
  throw new Error("usage: serve-smoke-registry.mjs <config.json> <ready-file>")
}

const configured = JSON.parse(readFileSync(configPath, "utf8"))
const packages = new Map(
  configured.map(({ manifest, tarball }, index) => {
    const pkg = JSON.parse(readFileSync(manifest, "utf8"))
    const bytes = readFileSync(tarball)
    return [
      pkg.name,
      {
        index,
        pkg,
        bytes,
        integrity: `sha512-${createHash("sha512").update(bytes).digest("base64")}`,
        shasum: createHash("sha1").update(bytes).digest("hex"),
      },
    ]
  }),
)

function sendJson(response, status, body) {
  const bytes = Buffer.from(`${JSON.stringify(body)}\n`)
  response.writeHead(status, {
    "content-length": bytes.length,
    "content-type": "application/json",
  })
  response.end(bytes)
}

const server = createServer((request, response) => {
  const requestUrl = new URL(request.url, "http://127.0.0.1")
  const tarballMatch = requestUrl.pathname.match(/^\/-\/tarballs\/(\d+)\.tgz$/)
  if (tarballMatch) {
    const entry = [...packages.values()].find(({ index }) => index === Number(tarballMatch[1]))
    if (!entry) return sendJson(response, 404, { error: "tarball not found" })
    response.writeHead(200, {
      "content-length": entry.bytes.length,
      "content-type": "application/octet-stream",
    })
    return response.end(entry.bytes)
  }

  const packageName = decodeURIComponent(requestUrl.pathname.slice(1).replace(/\/$/, ""))
  const entry = packages.get(packageName)
  if (!entry) return sendJson(response, 404, { error: "package not found" })

  const address = server.address()
  if (!address || typeof address === "string") throw new Error("registry has no TCP address")
  const tarball = `http://127.0.0.1:${address.port}/-/tarballs/${entry.index}.tgz`
  const version = {
    ...entry.pkg,
    _id: `${entry.pkg.name}@${entry.pkg.version}`,
    dist: { integrity: entry.integrity, shasum: entry.shasum, tarball },
  }
  return sendJson(response, 200, {
    name: entry.pkg.name,
    "dist-tags": { latest: entry.pkg.version },
    versions: { [entry.pkg.version]: version },
  })
})

server.listen(0, "127.0.0.1", () => {
  const address = server.address()
  if (!address || typeof address === "string") throw new Error("registry has no TCP address")
  writeFileSync(readyPath, `http://127.0.0.1:${address.port}/\n`)
})

process.on("SIGTERM", () => server.close())
process.on("SIGINT", () => server.close())
