/**
 * Small static guard for the public TypeScript package boundaries.
 * Runtime, declaration, tarball, and native-embed behavior belongs
 * to the executable package-layout consumer; this file checks only rules a
 * source/manifest scan can prove reliably.
 */

import { readFileSync, readdirSync } from "node:fs"
import { dirname, join, relative, resolve } from "node:path"
import { fileURLToPath } from "node:url"

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..")

interface PackageManifest {
  dependencies?: Record<string, string>
  devDependencies?: Record<string, string>
  exports?: Record<string, unknown>
  optionalDependencies?: Record<string, string>
  peerDependencies?: Record<string, string>
}

interface Rule {
  packageName: string
  forbiddenDependencies: Array<string | RegExp>
  forbiddenSource: RegExp
}

const rules: Rule[] = [
  {
    packageName: "bindings",
    forbiddenDependencies: [
      /^@aws-sdk\//,
      /^@google-cloud\//,
      /^@azure\//,
      "@grpc/grpc-js",
      "nice-grpc",
    ],
    forbiddenSource:
      /@aws-sdk\/|@google-cloud\/|@azure\/|@grpc\/grpc-js|nice-grpc|ALIEN_BINDINGS_GRPC_ADDRESS|ALIEN_BINDINGS_MODE|worker[-_/]protocol/i,
  },
  {
    packageName: "commands",
    forbiddenDependencies: ["@alienplatform/bindings", "@grpc/grpc-js", "nice-grpc"],
    forbiddenSource: /@alienplatform\/bindings|@grpc\/grpc-js|nice-grpc|worker[-_/]protocol/i,
  },
]

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) return sourceFiles(path)
    return /\.(?:c|m)?(?:j|t)sx?$/.test(entry.name) ? [path] : []
  })
}

function manifestFor(packageName: string): PackageManifest {
  return JSON.parse(
    readFileSync(join(repoRoot, "packages", packageName, "package.json"), "utf8"),
  ) as PackageManifest
}

function allDependencies(manifest: PackageManifest): string[] {
  return Object.keys({
    ...manifest.dependencies,
    ...manifest.devDependencies,
    ...manifest.optionalDependencies,
    ...manifest.peerDependencies,
  })
}

function matches(value: string, matcher: string | RegExp): boolean {
  return typeof matcher === "string" ? value === matcher : matcher.test(value)
}

const failures: string[] = []

for (const rule of rules) {
  const manifest = manifestFor(rule.packageName)
  for (const dependency of allDependencies(manifest)) {
    if (rule.forbiddenDependencies.some(matcher => matches(dependency, matcher))) {
      failures.push(`@alienplatform/${rule.packageName} depends on forbidden '${dependency}'`)
    }
  }

  const sourceRoot = join(repoRoot, "packages", rule.packageName, "src")
  for (const file of sourceFiles(sourceRoot)) {
    if (relative(sourceRoot, file).split(/[\\/]/).includes("generated")) {
      failures.push(
        `@alienplatform/${rule.packageName} ships generated protocol code in ${relative(repoRoot, file)}`,
      )
    }
    const source = readFileSync(file, "utf8")
    if (rule.forbiddenSource.test(source)) {
      failures.push(
        `@alienplatform/${rule.packageName} contains a forbidden reference in ${relative(repoRoot, file)}`,
      )
    }
  }
}

const sdkManifest = manifestFor("sdk")
if (sdkManifest.exports && Object.hasOwn(sdkManifest.exports, "./commands")) {
  failures.push("@alienplatform/sdk still exports the deleted './commands' subpath")
}

const sdkSource = join(repoRoot, "packages", "sdk", "src")
for (const file of sourceFiles(sdkSource)) {
  const workerRuntimeRoot = join(sdkSource, "worker-runtime")
  if (file.startsWith(`${workerRuntimeRoot}/`)) continue
  if (relative(sdkSource, file).split(/[\\/]/).includes("generated")) {
    failures.push(
      `@alienplatform/sdk ships generated protocol code outside ./worker-runtime in ${relative(repoRoot, file)}`,
    )
  }
  const source = readFileSync(file, "utf8")
  if (
    /(?:from\s+|import\s*\(|require\s*\()["'][^"']*(?:@grpc\/grpc-js|nice-grpc|worker[-_/]protocol)/i.test(
      source,
    )
  ) {
    failures.push(
      `@alienplatform/sdk references Worker protocol code outside ./worker-runtime in ${relative(repoRoot, file)}`,
    )
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL: ${failure}`)
  process.exitCode = 1
} else {
  console.log("package-layout static boundaries: PASS")
}
