"use client"

import { useState, useRef, useEffect } from "react"
import { useRouter } from "next/navigation"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { IconRocket, IconPlus, IconCheck, IconLoader2, IconAlertCircle } from "@tabler/icons-react"
import { PlatformSelector, type Platform } from "./platform-selector"
import { DeploymentMethod } from "./deployment-method"
import { Confetti, type ConfettiRef } from "@/components/ui/confetti"
import { match } from "ts-pattern"

// Agent status type (subset of platform agent statuses)
type AgentStatus = 
  | "pending"
  | "initial-setup" 
  | "provisioning"
  | "running"
  | "update-pending"
  | "updating"
  | "delete-pending"
  | "deleting"
  | "initial-setup-failed"
  | "provisioning-failed"
  | "update-failed"
  | "delete-failed"

interface DeployAgentDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  token?: string
  tokenLoading?: boolean
  tokenError?: string
  currentAgentCount: number
}

type DeploymentState = "configuring" | "deploying" | "completed" | "failed"

interface DeployingAgent {
  id: string
  name: string
  status: AgentStatus
  platform: string
}

export function DeployAgentDialog({ 
  open, 
  onOpenChange, 
  token, 
  tokenLoading = false,
  tokenError,
  currentAgentCount
}: DeployAgentDialogProps) {
  const router = useRouter()
  const [selectedPlatform, setSelectedPlatform] = useState<Platform>("local")
  const [deploymentState, setDeploymentState] = useState<DeploymentState>("configuring")
  const [deployingAgent, setDeployingAgent] = useState<DeployingAgent | null>(null)
  const [previousAgentCount, setPreviousAgentCount] = useState(currentAgentCount)
  const prevOpenRef = useRef(open)
  const confettiRef = useRef<ConfettiRef>(null)

  // Reset state when dialog opens/closes, track initial agent count
  useEffect(() => {
    const wasClosedNowOpen = !prevOpenRef.current && open
    const wasOpenNowClosed = prevOpenRef.current && !open
    
    if (wasClosedNowOpen) {
      // Dialog just opened - initialize
      setPreviousAgentCount(currentAgentCount)
      setDeploymentState("configuring")
      setDeployingAgent(null)
    } else if (wasOpenNowClosed) {
      // Dialog just closed - reset
      setDeploymentState("configuring")
      setDeployingAgent(null)
    }
    
    prevOpenRef.current = open
  }, [open, currentAgentCount])

  // Poll for new agent (only in configuring state, before deployment starts)
  useEffect(() => {
    if (!open || deploymentState !== "configuring") return

    const pollInterval = setInterval(async () => {
      try {
        const response = await fetch("/api/agents")
        if (!response.ok) return
        
        const data = await response.json()
        const agents = data.agents || []
        
        // Detect new agent (newest first)
        if (agents.length > previousAgentCount) {
          const newestAgent = agents[0]
          setDeployingAgent(newestAgent)
          setDeploymentState("deploying")
        }
      } catch (error) {
        console.error("Failed to poll agents:", error)
      }
    }, 2000) // Poll every 2 seconds

    return () => clearInterval(pollInterval)
  }, [open, deploymentState, previousAgentCount])

  // Track deploying agent status (poll for status updates)
  useEffect(() => {
    if (!open || deploymentState !== "deploying" || !deployingAgent) return

    const pollInterval = setInterval(async () => {
      try {
        const response = await fetch("/api/agents")
        if (!response.ok) return
        
        const data = await response.json()
        const agents = data.agents || []
        const updatedAgent = agents.find((a: DeployingAgent) => a.id === deployingAgent.id)
        
        if (updatedAgent) {
          setDeployingAgent(updatedAgent)
          
          // Check if deployment completed successfully
          if (updatedAgent.status === "running") {
            setDeploymentState("completed")
            confettiRef.current?.fire({
              particleCount: 100,
              spread: 70,
              origin: { y: 0.6 }
            })
          } 
          // Check if deployment failed
          else if (
            updatedAgent.status.includes("failed") || 
            updatedAgent.status === "delete-failed" ||
            updatedAgent.status === "update-failed" ||
            updatedAgent.status === "initial-setup-failed" ||
            updatedAgent.status === "provisioning-failed"
          ) {
            setDeploymentState("failed")
          }
        }
      } catch (error) {
        console.error("Failed to poll agent status:", error)
      }
    }, 2000) // Poll every 2 seconds

    return () => clearInterval(pollInterval)
  }, [open, deploymentState, deployingAgent])

  const handleClose = () => {
    onOpenChange(false)
    // Ensure we're on the agents page
    router.push("/agents")
  }

  const handleAddIntegration = () => {
    onOpenChange(false)
    router.push("/integrations")
  }

  const getStatusProgress = (status: AgentStatus): number => {
    return match(status)
      .with("pending", () => 20)
      .with("initial-setup", () => 40)
      .with("provisioning", () => 60)
      .with("running", () => 100)
      .otherwise(() => 0)
  }

  const getStatusLabel = (status: AgentStatus): string => {
    return match(status)
      .with("pending", () => "Initializing agent...")
      .with("initial-setup", () => "Setting up infrastructure...")
      .with("provisioning", () => "Deploying resources...")
      .with("running", () => "Agent is running!")
      .otherwise(() => status)
  }

  const renderContent = () => {
    if (tokenError) {
      return (
        <div className="py-8 px-4 text-center">
          <div className="mb-4 text-destructive">
            <p className="font-medium">{tokenError}</p>
          </div>
          <p className="text-sm text-muted-foreground">
            Start the dev server with: <code className="bg-muted px-2 py-1 rounded">alien dev server</code>
          </p>
        </div>
      )
    }

    if (deploymentState === "completed" && deployingAgent) {
      return (
        <div className="py-8 px-4 text-center space-y-6">
          <div className="relative mb-2">
            <div className="absolute inset-0 bg-green-500/20 rounded-full blur-xl" />
            <div className="relative flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-green-500/10 to-green-500/5 ring-1 ring-green-500/20 mx-auto">
              <IconCheck className="h-10 w-10 text-green-600" />
            </div>
          </div>
          <div>
            <h3 className="text-xl font-semibold mb-2">🎉 Agent deployed successfully!</h3>
            <p className="text-muted-foreground max-w-md mx-auto mb-2">
              <strong>{deployingAgent.name}</strong> is running in your environment.
            </p>
            <p className="text-muted-foreground max-w-md mx-auto mb-6">
              Add a GitHub integration to start analyzing your code reviews.
            </p>
          </div>
          <div className="flex gap-3 justify-center">
            <Button variant="outline" onClick={handleClose}>
              View Agents
            </Button>
            <Button onClick={handleAddIntegration}>
              <IconPlus className="mr-2 h-4 w-4" />
              Add Integration
            </Button>
          </div>
        </div>
      )
    }

    if (deploymentState === "failed" && deployingAgent) {
      return (
        <div className="py-8 px-4 text-center space-y-6">
          <div className="relative mb-2">
            <div className="absolute inset-0 bg-red-500/20 rounded-full blur-xl" />
            <div className="relative flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-red-500/10 to-red-500/5 ring-1 ring-red-500/20 mx-auto">
              <IconAlertCircle className="h-10 w-10 text-red-600" />
            </div>
          </div>
          <div>
            <h3 className="text-xl font-semibold mb-2">Deployment failed</h3>
            <p className="text-muted-foreground max-w-md mx-auto mb-6">
              There was an issue deploying <strong>{deployingAgent.name}</strong>. Check the agents page for details.
            </p>
          </div>
          <div className="flex gap-3 justify-center">
            <Button onClick={handleClose}>
              View Agents
            </Button>
          </div>
        </div>
      )
    }

    if (deploymentState === "deploying" && deployingAgent) {
      const progress = getStatusProgress(deployingAgent.status)
      const statusLabel = getStatusLabel(deployingAgent.status)

      return (
        <div className="py-8 px-4 space-y-6">
          <div className="text-center space-y-4">
            <div className="relative mb-2">
              <div className="absolute inset-0 bg-primary/20 rounded-full blur-xl animate-pulse" />
              <div className="relative flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-primary/10 to-primary/5 ring-1 ring-primary/20 mx-auto">
                <IconLoader2 className="h-10 w-10 text-primary animate-spin" />
              </div>
            </div>
            <div>
              <h3 className="text-xl font-semibold mb-2">Deploying agent...</h3>
              <p className="text-muted-foreground mb-1">
                <strong>{deployingAgent.name}</strong>
              </p>
              <Badge variant="outline" className="gap-1.5">
                {statusLabel}
              </Badge>
            </div>
          </div>
          
          <div className="space-y-2">
            <div className="flex justify-between text-sm">
              <span className="text-muted-foreground">Progress</span>
              <span className="font-medium">{progress}%</span>
            </div>
            <Progress value={progress} className="h-2" />
          </div>

          <div className="rounded-lg bg-muted/50 p-4 text-sm text-muted-foreground text-center">
            This usually takes 30-60 seconds. The dialog will update automatically when deployment completes.
          </div>
        </div>
      )
    }

    // Default: configuring state
    return (
      <div className="space-y-6 py-4">
        {/* Step 1: Platform selection */}
        <div className="space-y-4">
          <div className="flex items-center gap-3">
            <div className="flex items-center justify-center w-7 h-7 rounded-full bg-primary text-primary-foreground text-sm font-bold shadow-sm">
              1
            </div>
            <h3 className="text-base font-semibold">Choose Your Platform</h3>
          </div>
          
          <PlatformSelector
            selected={selectedPlatform}
            onSelect={setSelectedPlatform}
          />
        </div>

        {/* Step 2: Deployment Method */}
        <div className="space-y-4 pt-2">
          <div className="flex items-start gap-3">
            <div className="flex items-center justify-center w-7 h-7 rounded-full bg-primary text-primary-foreground text-sm font-bold shadow-sm shrink-0">
              2
            </div>
            <div className="flex-1 min-w-0">
              {tokenLoading ? (
                <div className="py-8 text-center text-muted-foreground">
                  Creating deployment group...
                </div>
              ) : (
                <DeploymentMethod platform={selectedPlatform} token={token} />
              )}
            </div>
          </div>
        </div>
      </div>
    )
  }

  const getDialogTitle = () => {
    return match(deploymentState)
      .with("deploying", () => "Deploying Agent")
      .with("completed", () => "Agent Deployed!")
      .with("failed", () => "Deployment Failed")
      .otherwise(() => "Deploy an Agent")
  }

  const getDialogDescription = () => {
    return match(deploymentState)
      .with("deploying", () => "Please wait while we set up your agent")
      .with("completed", () => "Your agent is running and ready to use")
      .with("failed", () => "The deployment encountered an error")
      .otherwise(() => "Choose your deployment platform and follow the instructions")
  }

  return (
    <>
      <Confetti
        ref={confettiRef}
        className="pointer-events-none fixed left-0 top-0 z-[100] size-full"
      />
      <Dialog open={open} onOpenChange={deploymentState === "configuring" ? onOpenChange : undefined}>
        <DialogContent className="sm:max-w-4xl max-h-[100vh] overflow-y-auto">
          <DialogHeader>
            <div className="flex items-center gap-3">
              <div className="p-2.5 rounded-xl bg-primary/10 ring-1 ring-primary/20">
                <IconRocket className="h-6 w-6 text-primary" />
              </div>
              <div>
                <DialogTitle className="text-xl">{getDialogTitle()}</DialogTitle>
                <DialogDescription>{getDialogDescription()}</DialogDescription>
              </div>
            </div>
          </DialogHeader>
          {renderContent()}
        </DialogContent>
      </Dialog>
    </>
  )
}

