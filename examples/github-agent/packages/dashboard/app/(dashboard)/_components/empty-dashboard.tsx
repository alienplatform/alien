"use client"

import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { useAgents, useDeploymentGroup } from "@/lib/queries"
import { IconArrowRight, IconBrandGithub, IconPlus, IconServer } from "@tabler/icons-react"
import Link from "next/link"
import { useState } from "react"
import { DeployAgentDialog } from "../agents/_components/deploy-agent-dialog"

interface EmptyDashboardProps {
  hasAgents: boolean
}

export function EmptyDashboard({ hasAgents }: EmptyDashboardProps) {
  const [deployDialogOpen, setDeployDialogOpen] = useState(false)
  const { data: agents = [] } = useAgents()
  const {
    data: deploymentGroupData,
    isLoading: tokenLoading,
    error: tokenError,
  } = useDeploymentGroup()

  if (!hasAgents) {
    // No agents - prompt to deploy first
    return (
      <>
        <div className="flex flex-1 flex-col gap-4 p-4 md:gap-6 md:p-6">
          <div className="flex flex-col gap-1">
            <h1 className="text-2xl font-bold tracking-tight">Dashboard</h1>
            <p className="text-muted-foreground">
              Overview of your code review analytics across all integrations.
            </p>
          </div>
          <Card className="flex-1">
            <CardContent className="flex flex-col items-center justify-center py-16">
              <div className="relative mb-6">
                <div className="absolute inset-0 bg-muted-foreground/10 rounded-full blur-xl" />
                <div className="relative flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-muted to-muted/50 ring-1 ring-border">
                  <IconServer className="h-10 w-10 text-muted-foreground" />
                </div>
              </div>
              <h3 className="text-xl font-semibold mb-2">No agents deployed</h3>
              <p className="text-muted-foreground text-center max-w-md mb-6">
                Deploy an agent first to start analyzing your GitHub repositories. Agents run in
                your environment, keeping your data private and secure.
              </p>
              <Button asChild>
                <Link href="/agents?deploy=true">
                  <IconPlus className="mr-2 h-4 w-4" />
                  Deploy Agent
                </Link>
              </Button>
            </CardContent>
          </Card>
        </div>
      </>
    )
  }

  // Has agents but no integrations
  return (
    <div className="flex flex-1 flex-col gap-4 p-4 md:gap-6 md:p-6">
      <div className="flex flex-col gap-1">
        <h1 className="text-2xl font-bold tracking-tight">Dashboard</h1>
        <p className="text-muted-foreground">
          Overview of your code review analytics across all integrations.
        </p>
      </div>
      <Card className="flex-1">
        <CardContent className="flex flex-col items-center justify-center py-16">
          <div className="relative mb-6">
            <div className="absolute inset-0 bg-primary/20 rounded-full blur-xl" />
            <div className="relative flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-primary/10 to-primary/5 ring-1 ring-primary/20">
              <IconBrandGithub className="h-10 w-10 text-primary" />
            </div>
          </div>
          <h3 className="text-xl font-semibold mb-2">🎉 Agent deployed!</h3>
          <p className="text-muted-foreground text-center max-w-md mb-6">
            Great! Your agent is running. Now connect a GitHub repository to start analyzing code
            review metrics and track team performance.
          </p>
          <Button asChild>
            <Link href="/integrations">
              <IconPlus className="mr-2 h-4 w-4" />
              Add Integration
            </Link>
          </Button>
        </CardContent>
      </Card>
    </div>
  )
}
