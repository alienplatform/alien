import { defineConfig } from "@kubb/core"
import { pluginOas } from "@kubb/plugin-oas"
import { pluginZod } from "@kubb/plugin-zod"

export default defineConfig({
  root: ".",
  input: {
    path: "./openapi.json",
  },
  output: {
    path: "./src/generated",
    extension: {
      ".ts": ".js",
    },
  },
  plugins: [
    pluginOas(),
    pluginZod({
      inferred: true,
      version: "4",
      transformers: {
        name: (name, type) => {
          if (type === "function") {
            return name.charAt(0).toUpperCase() + name.slice(1)
          }
          if (type === "type") {
            return (
              name
                .replace(/Schema$/, "")
                .charAt(0)
                .toUpperCase() + name.replace(/Schema$/, "").slice(1)
            )
          }
          if (type === "file") {
            // Convert camelCase to kebab-case
            return name.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase()
          }
          return name
        },
      },
    }),
  ],
})
