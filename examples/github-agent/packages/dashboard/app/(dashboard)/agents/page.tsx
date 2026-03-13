"use client"

import { useState, useEffect } from "react"
import { useSearchParams } from "next/navigation"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  IconServer,
  IconPlus,
  IconCloud,
} from "@tabler/icons-react"
import Image from "next/image"
import { match } from "ts-pattern"
import { DeployAgentDialog } from "./_components/deploy-agent-dialog"
import { useDeploymentGroup, useAgents } from "@/lib/queries"

interface Agent {
  id: string
  name: string
  status: string
  platform: string
}

const platformIcons: Record<string, string> = {
  aws: "/aws.svg",
  gcp: "/google-cloud.svg",
  azure: "/azure.svg",
  kubernetes: "/kubernetes.svg",
  local: "/local.svg",
}

export default function AgentsPage() {
  const searchParams = useSearchParams()
  const [deployDialogOpen, setDeployDialogOpen] = useState(false)
  
  const { data: agents = [], isLoading: agentsLoading } = useAgents()
  const { 
    data: deploymentGroupData,
    isLoading: tokenLoading,
    error: tokenError,
  } = useDeploymentGroup()

  // Support opening deploy dialog via URL parameter (?deploy=true)
  useEffect(() => {
    if (searchParams.get('deploy') === 'true') {
      setDeployDialogOpen(true)
    }
  }, [searchParams])

  const getStatusColor = (status: string) => {
    return match(status.toLowerCase())
      .with("running", "online", () => "bg-green-500" as const)
      .with("starting", "deploying", () => "bg-yellow-500" as const)
      .with("stopped", "offline", () => "bg-gray-500" as const)
      .with("error", () => "bg-red-500" as const)
      .otherwise(() => "bg-gray-500" as const)
  }

  const renderPlatformIcon = (platform: string) => {
    const iconSrc = platformIcons[platform]
    if (iconSrc) {
      const needsDarkFilter = platform === "local"
      return (
        <Image
          src={iconSrc}
          alt={platform}
          width={20}
          height={20}
          className={`object-contain ${needsDarkFilter ? "dark:brightness-100 brightness-0" : ""}`}
        />
      )
    }
    
    return <IconCloud className="h-5 w-5 text-muted-foreground" />
  }

  return (
    <div className="@container/main flex flex-1 flex-col gap-4 p-4 md:gap-6 md:p-6">
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-bold tracking-tight">Agents</h1>
          <p className="text-muted-foreground">
            Deploy and manage agents that run in your environment.
          </p>
        </div>
        <Button onClick={() => setDeployDialogOpen(true)}>
          <IconPlus className="mr-2 h-4 w-4" />
          Deploy Agent
        </Button>
      </div>

      {/* Active agents */}
      <div className="space-y-4">
        <h2 className="text-lg font-semibold flex items-center gap-2">
          <IconServer className="h-5 w-5" />
          Active Agents
          {!agentsLoading && agents.length > 0 && (
            <Badge variant="secondary" className="ml-2">
              {agents.length}
            </Badge>
          )}
        </h2>
        
        {agentsLoading ? (
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {[...Array(3)].map((_, i) => (
              <Card key={`skeleton-${i}`}>
                <CardHeader>
                  <div className="flex items-center gap-3">
                    <Skeleton className="h-10 w-10 rounded-full" />
                    <div className="space-y-2 flex-1">
                      <Skeleton className="h-4 w-32" />
                      <Skeleton className="h-3 w-24" />
                    </div>
                  </div>
                </CardHeader>
              </Card>
            ))}
          </div>
        ) : agents.length === 0 ? (
          <Card>
            <CardContent className="flex flex-col items-center justify-center py-16">
              <div className="relative mb-6">
                <div className="absolute inset-0 bg-muted-foreground/10 rounded-full blur-xl" />
                <div className="relative flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-muted to-muted/50 ring-1 ring-border">
                  <IconServer className="h-10 w-10 text-muted-foreground" />
                </div>
              </div>
              <h3 className="text-xl font-semibold mb-2">No agents running</h3>
              <p className="text-muted-foreground text-center max-w-md mb-6">
                Deploy your first agent to start analyzing repositories. Agents run in your environment, keeping your data private and secure.
              </p>
              <Button onClick={() => setDeployDialogOpen(true)}>
                <IconPlus className="mr-2 h-4 w-4" />
                Deploy Agent
              </Button>
            </CardContent>
          </Card>
        ) : (
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {agents.map((agent: Agent) => (
              <Card key={agent.id} className="relative overflow-hidden group">
                <div className="absolute top-0 right-0 w-32 h-32 bg-green-500/5 rounded-full blur-3xl opacity-0 group-hover:opacity-100 transition-opacity" />
                <CardHeader>
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-3">
                      <div className="flex h-10 w-10 items-center justify-center rounded-full bg-primary/10 ring-1 ring-primary/20">
                        {renderPlatformIcon(agent.platform)}
                      </div>
                      <div>
                        <CardTitle className="text-base font-semibold">
                          {agent.name || agent.id}
                        </CardTitle>
                        <CardDescription className="flex items-center gap-2 mt-1">
                          <Badge className={`${getStatusColor(agent.status)} hover:${getStatusColor(agent.status)} gap-1`}>
                            <div className="h-1.5 w-1.5 rounded-full bg-white animate-pulse" />
                            {agent.status}
                          </Badge>
                          <Badge variant="outline" className="capitalize">
                            {agent.platform}
                          </Badge>
                        </CardDescription>
                      </div>
                    </div>
                  </div>
                </CardHeader>
              </Card>
            ))}
          </div>
        )}
      </div>

      {/* Deploy Agent Dialog */}
      <DeployAgentDialog
        open={deployDialogOpen}
        onOpenChange={setDeployDialogOpen}
        token={deploymentGroupData?.deploymentToken}
        tokenLoading={tokenLoading}
        tokenError={tokenError ? String(tokenError) : undefined}
        currentAgentCount={agents.length}
      />
    </div>
  )
}
