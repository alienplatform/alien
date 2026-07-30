import type { NextConfig } from "next"

const nextConfig: NextConfig = {
  // The container runs the generated .next/standalone/server.js without node_modules.
  output: "standalone",
  // Both packages locate their native half (napi addon / gateway binary) with dynamic
  // requires the bundler cannot resolve; keep them external and let file tracing copy
  // them into the standalone output.
  serverExternalPackages: ["@alienplatform/bindings", "@alienplatform/ai-gateway"],
  // The per-platform prebuild packages are resolved with computed specifiers file
  // tracing cannot see; without this the standalone output ships no native addon.
  outputFileTracingIncludes: {
    "/api/**": [
      "node_modules/@alienplatform/bindings-*/**",
      "node_modules/@alienplatform/ai-gateway-*/**",
    ],
  },
}

export default nextConfig
