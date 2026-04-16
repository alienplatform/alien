import { defineConfig } from "tsdown"

export default defineConfig({
  entry: ["src/index.ts", "src/commands/index.ts"],
  // Use tsc for declaration files instead of tsdown's built-in dts.
  // With two entry points that share types, tsdown's rolldown-based dts
  // generation code-splits the .d.ts output, putting shared types into
  // index.d.ts (as a chunk with minified names) and the actual main entry
  // types into index2.d.ts. This breaks the package.json "types" field.
  dts: false,
  hash: false,
  ignoreWatch: ".turbo",
  // Bundle all dependencies so the package is self-contained.
  // This allows bun build --compile to work without needing
  // transitive dependencies installed in the user's project.
  noExternal: [/.*/],
})
