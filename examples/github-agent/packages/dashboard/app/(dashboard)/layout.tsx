import { headers } from "next/headers"
import { redirect } from "next/navigation"
import { auth } from "@/lib/auth"
import { db } from "@/lib/db"
import { member, organization } from "@/lib/schema"
import { eq } from "drizzle-orm"
import { SidebarProvider, SidebarInset, SidebarTrigger } from "@/components/ui/sidebar"
import { AppSidebar } from "@/components/app-sidebar"
import { Separator } from "@/components/ui/separator"
import { Toaster } from "@/components/ui/sonner"
import { DynamicBreadcrumb } from "@/components/dynamic-breadcrumb"

export default async function DashboardLayout({
  children,
}: {
  children: React.ReactNode
}) {
  const session = await auth.api.getSession({
    headers: await headers(),
  })

  if (!session) {
    redirect("/login")
  }

  // Get user's organization memberships
  const userMemberships = await db
    .select({
      organizationId: member.organizationId,
      role: member.role,
      orgName: organization.name,
      orgSlug: organization.slug,
      orgLogo: organization.logo,
      orgId: organization.id,
    })
    .from(member)
    .innerJoin(organization, eq(member.organizationId, organization.id))
    .where(eq(member.userId, session.user.id))

  if (userMemberships.length === 0) {
    redirect("/onboarding")
  }

  // Format organizations for the sidebar
  const organizations = userMemberships.map(m => ({
    id: m.orgId,
    name: m.orgName,
    slug: m.orgSlug,
    logo: m.orgLogo,
  }))

  return (
    <SidebarProvider
      style={
        {
          "--sidebar-width": "calc(var(--spacing) * 64)",
          "--header-height": "calc(var(--spacing) * 12)",
        } as React.CSSProperties
      }
    >
      <AppSidebar
        variant="inset"
        user={{
          name: session.user.name,
          email: session.user.email,
          avatar: session.user.image || undefined,
        }}
        organizations={organizations}
        activeOrganizationId={session.session.activeOrganizationId}
      />
      <SidebarInset>
        <header className="flex h-[--header-height] shrink-0 items-center gap-2 transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-12 border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60">
          <div className="flex items-center gap-2 px-4 py-2">
            <SidebarTrigger className="-ml-1" />
            <Separator
              orientation="vertical"
              className="mr-2 data-[orientation=vertical]:h-4"
            />
            <DynamicBreadcrumb />
          </div>
        </header>
        <main className="flex flex-1 flex-col">
          {children}
        </main>
      </SidebarInset>
      <Toaster />
    </SidebarProvider>
  )
}

