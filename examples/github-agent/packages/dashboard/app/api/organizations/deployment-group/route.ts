import { auth } from "@/lib/auth"
import { db } from "@/lib/db"
import { getOrCreateDeploymentGroup } from "@/lib/deployment-groups"
import { member, organization } from "@/lib/schema"
import { and, eq } from "drizzle-orm"
import { headers } from "next/headers"
import { NextResponse } from "next/server"

/**
 * GET /api/organizations/deployment-group
 *
 * Gets or creates a deployment group for the user's active organization.
 * Returns the deployment group ID and token.
 *
 * In local dev: Returns the default local dev deployment group
 * In production: Creates a deployment group via the Platform API
 */
export async function GET() {
  try {
    const session = await auth.api.getSession({
      headers: await headers(),
    })

    if (!session) {
      return NextResponse.json({ error: "Unauthorized" }, { status: 401 })
    }

    const activeOrgId = session.session.activeOrganizationId

    if (!activeOrgId) {
      return NextResponse.json({ error: "No active organization" }, { status: 400 })
    }

    // Verify user is a member of the organization
    const membership = await db
      .select()
      .from(member)
      .where(and(eq(member.organizationId, activeOrgId), eq(member.userId, session.user.id)))
      .limit(1)

    if (membership.length === 0) {
      return NextResponse.json({ error: "Not a member of this organization" }, { status: 403 })
    }

    // Get organization details
    const org = await db
      .select()
      .from(organization)
      .where(eq(organization.id, activeOrgId))
      .limit(1)

    if (org.length === 0) {
      return NextResponse.json({ error: "Organization not found" }, { status: 404 })
    }

    // Get or create deployment group
    const { deploymentGroupId, deploymentToken } = await getOrCreateDeploymentGroup(
      activeOrgId,
      org[0].name,
      org[0].slug,
    )

    return NextResponse.json({
      deploymentGroupId,
      deploymentToken,
      deploymentLink: `https://alien.dev/deploy#${deploymentToken}`,
    })
  } catch (error) {
    console.error("Failed to get/create deployment group:", error)
    return NextResponse.json({ error: "Internal server error" }, { status: 500 })
  }
}
