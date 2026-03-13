"use client"

import { usePathname } from "next/navigation"
import Link from "next/link"
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb"
import {
  IconDashboard,
  IconServer,
  IconPlug,
  IconGitPullRequest,
} from "@tabler/icons-react"
import { match } from "ts-pattern"

export function DynamicBreadcrumb() {
  const pathname = usePathname()

  const { icon: Icon, label, parent } = match(pathname)
    .with("/", () => ({ 
      icon: IconDashboard, 
      label: "Dashboard", 
      parent: null 
    }))
    .with("/agents", () => ({ 
      icon: IconServer, 
      label: "Agents", 
      parent: null 
    }))
    .with("/integrations", () => ({ 
      icon: IconPlug, 
      label: "Integrations", 
      parent: null 
    }))
    .with("/pull-requests", () => ({ 
      icon: IconGitPullRequest, 
      label: "Pull Requests", 
      parent: { href: "/", label: "Dashboard" } 
    }))
    .otherwise(() => ({ 
      icon: IconDashboard, 
      label: "Dashboard", 
      parent: null 
    }))

  if (!parent) {
    return (
      <Breadcrumb>
        <BreadcrumbList>
          <BreadcrumbItem>
            <BreadcrumbPage className="flex items-center gap-2">
              <Icon className="h-4 w-4" />
              {label}
            </BreadcrumbPage>
          </BreadcrumbItem>
        </BreadcrumbList>
      </Breadcrumb>
    )
  }

  return (
    <Breadcrumb>
      <BreadcrumbList>
        <BreadcrumbItem>
          <BreadcrumbLink asChild>
            <Link href={parent.href}>{parent.label}</Link>
          </BreadcrumbLink>
        </BreadcrumbItem>
        <BreadcrumbSeparator />
        <BreadcrumbItem>
          <BreadcrumbPage className="flex items-center gap-2">
            <Icon className="h-4 w-4" />
            {label}
          </BreadcrumbPage>
        </BreadcrumbItem>
      </BreadcrumbList>
    </Breadcrumb>
  )
}

