import { auth } from "@/lib/auth"
import { alien, config } from "@/lib/config"
import { headers } from "next/headers"

type RouteContext = {
  params: Promise<{ id: string }>
}

/**
 * GET /api/agents/:id/info
 *
 * Get connection info for a specific agent.
 * Returns ARC endpoint URL and resource public URLs.
 */
export async function GET(_request: Request, context: RouteContext): Promise<Response> {
  const session = await auth.api.getSession({
    headers: await headers(),
  })

  if (!session) {
    return Response.json({ error: "Unauthorized" }, { status: 401 })
  }

  try {
    const { id } = await context.params
    const info = await alien.deployments.getInfo({
      workspace: config.workspace,
      id,
    })
    return Response.json(info)
  } catch (error) {
    console.error("Failed to get agent info:", error)
    return Response.json(
      { error: error instanceof Error ? error.message : "Failed to get agent info" },
      { status: 500 },
    )
  }
}
