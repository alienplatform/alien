import fs from "node:fs"

const packageFile = new URL("../typescript/package.json", import.meta.url)
const packageJson = JSON.parse(fs.readFileSync(packageFile, "utf8"))
if (!packageJson.exports?.["./models"]) {
  throw new Error("Generated manager package is missing its models export")
}

// Keep generated routes intact while restoring the published model aliases.
for (const [route, module] of [
  ["./models", "models"],
  ["./models/stacksettings", "stacksettings"],
  ["./models/stacksettings.js", "stacksettings"],
]) {
  packageJson.exports[route] = {
    source: `./src/compat/${module}.ts`,
    types: `./esm/compat/${module}.d.ts`,
    default: `./esm/compat/${module}.js`,
  }
}
fs.writeFileSync(packageFile, JSON.stringify(packageJson, null, 2) + "\n")

const jsrFile = new URL("../typescript/jsr.json", import.meta.url)
const jsrJson = JSON.parse(fs.readFileSync(jsrFile, "utf8"))
if (!jsrJson.exports?.["./models"]) {
  throw new Error("Generated JSR package is missing its models export")
}
jsrJson.exports["./models"] = "./src/compat/models.ts"
fs.writeFileSync(jsrFile, JSON.stringify(jsrJson, null, 2) + "\n")
