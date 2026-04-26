/**
 * GitHub Agent - Remote Agent Entrypoint
 *
 * Registers commands for GitHub analysis and exposes HTTP endpoints
 * for direct PR access.
 */
import { Hono } from "hono"
import { cors } from "hono/cors"
import { registerCommands } from "./commands.js"
import { registerEndpoints } from "./endpoints.js"

const app = new Hono()

// Allow all CORS requests for development
app.use("*", cors())

registerCommands()
registerEndpoints(app)

export default app

// Export schemas and types for use in the dashboard
export * from "./schemas.js"
