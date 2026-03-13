"use client"

import * as React from "react"
import { ChevronsUpDown, Plus } from "lucide-react"
import { IconBrandGithub } from "@tabler/icons-react"
import { useRouter } from "next/navigation"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"
import { authClient } from "@/lib/auth-client"

interface Organization {
  id: string
  name: string
  slug: string | null
  logo: string | null
}

interface TeamSwitcherProps {
  organizations: Organization[]
  activeOrganizationId: string | null
}

export function TeamSwitcher({ organizations, activeOrganizationId }: TeamSwitcherProps) {
  const { isMobile } = useSidebar()
  const router = useRouter()
  
  const activeOrg = organizations.find(org => org.id === activeOrganizationId) || organizations[0]

  if (!activeOrg) {
    return null
  }

  const handleSwitchOrganization = async (orgId: string) => {
    await authClient.organization.setActive({
      organizationId: orgId,
    })
    
    // Refresh the page to reload data for the new organization
    router.refresh()
  }

  const handleCreateOrganization = () => {
    router.push("/onboarding")
  }

  return (
    <SidebarMenu>
      <SidebarMenuItem>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <SidebarMenuButton
              size="lg"
              className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground"
            >
              <div className="bg-sidebar-primary text-sidebar-primary-foreground flex aspect-square size-8 items-center justify-center rounded-lg">
                {activeOrg.logo ? (
                  <img src={activeOrg.logo} alt={activeOrg.name} className="size-4 rounded" />
                ) : (
                  <IconBrandGithub className="size-4" />
                )}
              </div>
              <div className="grid flex-1 text-left text-sm leading-tight">
                <span className="truncate font-medium">{activeOrg.name}</span>
                <span className="truncate text-xs text-muted-foreground">
                  {organizations.length} {organizations.length === 1 ? 'organization' : 'organizations'}
                </span>
              </div>
              <ChevronsUpDown className="ml-auto" />
            </SidebarMenuButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            className="w-[--radix-dropdown-menu-trigger-width] min-w-56 rounded-lg"
            align="start"
            side={isMobile ? "bottom" : "right"}
            sideOffset={4}
          >
            <DropdownMenuLabel className="text-muted-foreground text-xs">
              Organizations
            </DropdownMenuLabel>
            {organizations.map((org, index) => (
              <DropdownMenuItem
                key={org.id}
                onClick={() => handleSwitchOrganization(org.id)}
                className="gap-2 p-2"
              >
                <div className="flex size-6 items-center justify-center rounded-md border">
                  {org.logo ? (
                    <img src={org.logo} alt={org.name} className="size-3.5 shrink-0 rounded" />
                  ) : (
                    <IconBrandGithub className="size-3.5 shrink-0" />
                  )}
                </div>
                <span className="flex-1 truncate">{org.name}</span>
                {index < 9 && <DropdownMenuShortcut>⌘{index + 1}</DropdownMenuShortcut>}
              </DropdownMenuItem>
            ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem className="gap-2 p-2" onClick={handleCreateOrganization}>
              <div className="flex size-6 items-center justify-center rounded-md border bg-transparent">
                <Plus className="size-4" />
              </div>
              <div className="text-muted-foreground font-medium">Create organization</div>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </SidebarMenuItem>
    </SidebarMenu>
  )
}

