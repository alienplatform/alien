"use client"

import * as React from "react"
import Link from "next/link"
import { usePathname } from "next/navigation"
import {
  IconBrandGithub,
  IconChartBar,
  IconDashboard,
  IconPlug,
  IconServer,
  IconSettings,
  IconHelp,
  IconLogout,
} from "@tabler/icons-react"

import { NavMain } from "@/components/nav-main"
import { NavSecondary } from "@/components/nav-secondary"
import { NavUser } from "@/components/nav-user"
import { TeamSwitcher } from "@/components/team-switcher"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"

interface Organization {
  id: string
  name: string
  slug: string | null
  logo: string | null
}

const navMain = [
  {
    title: "Dashboard",
    url: "/",
    icon: IconDashboard,
  },
  {
    title: "Agents",
    url: "/agents",
    icon: IconServer,
  },
  {
    title: "Integrations",
    url: "/integrations",
    icon: IconPlug,
  },
]

const navSecondary = [
  {
    title: "Settings",
    url: "/settings",
    icon: IconSettings,
  },
  {
    title: "Documentation",
    url: "https://docs.alien.dev",
    icon: IconHelp,
    external: true,
  },
]

interface AppSidebarProps extends React.ComponentProps<typeof Sidebar> {
  user?: {
    name: string
    email: string
    avatar?: string
  }
  organizations?: Organization[]
  activeOrganizationId?: string | null
}

export function AppSidebar({ user, organizations = [], activeOrganizationId, ...props }: AppSidebarProps) {
  const pathname = usePathname()

  const navMainWithActive = navMain.map((item) => ({
    ...item,
    isActive: pathname === item.url || (item.url !== "/" && pathname.startsWith(item.url)),
  }))

  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader>
        {organizations.length > 0 ? (
          <TeamSwitcher organizations={organizations} activeOrganizationId={activeOrganizationId} />
        ) : (
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton
                asChild
                className="data-[slot=sidebar-menu-button]:!p-1.5"
              >
                <Link href="/">
                  <div className="bg-primary text-primary-foreground flex size-7 items-center justify-center rounded-md">
                    <IconBrandGithub className="size-4" />
                  </div>
                  <span className="text-base font-semibold">Code Intelligence</span>
                </Link>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        )}
      </SidebarHeader>
      <SidebarContent>
        <NavMain items={navMainWithActive} />
        <NavSecondary items={navSecondary} className="mt-auto" />
      </SidebarContent>
      <SidebarFooter>
        <NavUser user={user || { name: "Demo User", email: "demo@example.com" }} />
      </SidebarFooter>
    </Sidebar>
  )
}
