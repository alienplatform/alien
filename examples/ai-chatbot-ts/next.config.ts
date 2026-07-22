import type { NextConfig } from "next"

const nextConfig: NextConfig = {
  // The container runs the generated .next/standalone/server.js without node_modules.
  output: "standalone",
}

export default nextConfig
