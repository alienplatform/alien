import { readFileSync, writeFileSync } from "node:fs"
import { resolve } from "node:path"
import { pathToFileURL } from "node:url"

// Release preparation must version runtime metadata as well as manifests.
// Keep the API and generator versions supplied by Speakeasy unchanged.
export function withSDKReleaseVersion(source, version) {
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`Invalid SDK release version: ${version}`)
  }
  const patterns = [/(\bsdkVersion: ")[^"]+("[,]?)/g, /("speakeasy-sdk\/typescript )\S+( )/g]
  let result = source
  for (const pattern of patterns) {
    if ([...result.matchAll(pattern)].length !== 1) {
      throw new Error("Expected exactly one SDK version and default user-agent in generated config")
    }
    result = result.replace(pattern, (_, prefix, suffix) => prefix + version + suffix)
  }
  return result
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  for (const sdk of ["platform", "manager"]) {
    const root = `client-sdks/${sdk}/typescript`
    const { version } = JSON.parse(readFileSync(`${root}/package.json`, "utf8"))
    const path = `${root}/src/lib/config.ts`
    const source = readFileSync(path, "utf8")
    const generated = withSDKReleaseVersion(source, version)
    if (process.argv.includes("--check")) {
      if (source !== generated)
        throw new Error(`${sdk} SDK runtime metadata does not match package version ${version}`)
    } else {
      writeFileSync(path, generated)
    }
  }
}
