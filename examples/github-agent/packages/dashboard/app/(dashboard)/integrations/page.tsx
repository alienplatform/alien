"use client"

import { useState, useEffect } from "react"
import { useRouter } from "next/navigation"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { toast } from "sonner"
import {
  IconBrandGithub,
  IconPlus,
  IconRefresh,
  IconLock,
  IconLoader2,
  IconServer,
  IconAlertCircle,
  IconSettings,
  IconAlertTriangle,
} from "@tabler/icons-react"
import Link from "next/link"
import { useIntegrations, useAgents, useAddIntegration, useAnalyzeIntegration, useUpdateIntegration } from "@/lib/queries"

interface Integration {
  id: string
  owner: string
  repo: string
  hasToken: boolean
  baseUrl?: string
  agentId?: string
  isActive: boolean
  createdAt: Date
}

export default function IntegrationsPage() {
  const router = useRouter()
  const [addDialogOpen, setAddDialogOpen] = useState(false)
  const [editDialogOpen, setEditDialogOpen] = useState(false)
  const [editingIntegration, setEditingIntegration] = useState<Integration | null>(null)
  const [owner, setOwner] = useState("")
  const [repo, setRepo] = useState("")
  const [token, setToken] = useState("")
  const [baseUrl, setBaseUrl] = useState("")
  const [selectedAgentId, setSelectedAgentId] = useState("")
  const [editSelectedAgentId, setEditSelectedAgentId] = useState("")

  const { data: integrations = [], isLoading: integrationsLoading } = useIntegrations()
  const { data: agents = [], isLoading: agentsLoading } = useAgents()
  const addIntegration = useAddIntegration()
  const analyzeIntegration = useAnalyzeIntegration()
  const updateIntegration = useUpdateIntegration()

  const loading = integrationsLoading || agentsLoading
  const isFirstIntegration = integrations.length === 0

  // Auto-open add integration dialog for first-time users (smooth onboarding flow)
  useEffect(() => {
    if (!loading && agents.length > 0 && integrations.length === 0) {
      const timer = setTimeout(() => {
        setAddDialogOpen(true)
      }, 800) // Slight delay so the page loads first
      return () => clearTimeout(timer)
    }
  }, [loading, agents.length, integrations.length])

  // Auto-select first agent when available
  useEffect(() => {
    if (agents.length > 0 && !selectedAgentId) {
      setSelectedAgentId(agents[0].id)
    }
  }, [agents, selectedAgentId])

  // Initialize edit agent selection when dialog opens
  useEffect(() => {
    if (editDialogOpen && editingIntegration && editingIntegration.agentId) {
      setEditSelectedAgentId(editingIntegration.agentId)
    } else if (editDialogOpen && agents.length > 0) {
      setEditSelectedAgentId(agents[0].id)
    }
  }, [editDialogOpen, editingIntegration, agents])

  const handleAddIntegration = async (e: React.FormEvent) => {
    e.preventDefault()
    
    if (!selectedAgentId) {
      toast.error("Please select an agent")
      return
    }

    const wasFirstIntegration = isFirstIntegration

    try {
      await addIntegration.mutateAsync({
        owner,
        repo,
        token,
        baseUrl,
        agentId: selectedAgentId,
      })
      
      toast.success("🎉 Integration added successfully!", {
        description: `${owner}/${repo} is now connected. ${wasFirstIntegration ? 'Redirecting to dashboard...' : 'Click "Analyze" to fetch metrics.'}`,
        duration: wasFirstIntegration ? 2000 : 5000,
      })
      setAddDialogOpen(false)
      setOwner("")
      setRepo("")
      setToken("")
      setBaseUrl("")

      // Redirect to dashboard after first integration
      if (wasFirstIntegration) {
        setTimeout(() => {
          router.push("/")
        }, 1500)
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to add integration")
    }
  }

  const handleAnalyze = (integration: Integration) => {
    if (!integration.isActive) {
      toast.error("Cannot analyze inactive integration. Please assign an agent first.")
      return
    }
    
    toast.promise(
      analyzeIntegration.mutateAsync(integration.id),
      {
        loading: `Analyzing ${integration.owner}/${integration.repo}...`,
        success: "Analysis complete! Check the dashboard for updated metrics.",
        error: "Analysis failed",
      }
    )
  }

  const handleEditIntegration = (integration: Integration) => {
    setEditingIntegration(integration)
    setEditDialogOpen(true)
  }

  const handleSaveEdit = async (e: React.FormEvent) => {
    e.preventDefault()
    
    if (!editingIntegration || !editSelectedAgentId) {
      toast.error("Please select an agent")
      return
    }

    try {
      await updateIntegration.mutateAsync({
        integrationId: editingIntegration.id,
        agentId: editSelectedAgentId,
      })
      
      toast.success("Integration updated successfully")
      setEditDialogOpen(false)
      setEditingIntegration(null)
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Failed to update integration")
    }
  }

  const hasNoAgents = agents.length === 0

  return (
    <div className="@container/main flex flex-1 flex-col gap-4 p-4 md:gap-6 md:p-6">
      <div className="flex items-center justify-between flex-wrap gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-2xl font-bold tracking-tight">Integrations</h1>
          <p className="text-muted-foreground">
            Connect GitHub repositories to analyze code review metrics.
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Dialog open={addDialogOpen} onOpenChange={setAddDialogOpen}>
            <DialogTrigger asChild>
              <Button disabled={hasNoAgents}>
                <IconPlus className="mr-2 h-4 w-4" />
                Add Integration
              </Button>
            </DialogTrigger>
            <DialogContent className="sm:max-w-md">
              <DialogHeader>
                <DialogTitle>Add GitHub Integration</DialogTitle>
                <DialogDescription>
                  Connect a GitHub repository to start analyzing pull requests.
                </DialogDescription>
              </DialogHeader>
              <form onSubmit={handleAddIntegration} className="space-y-4">
                {/* Agent Selection */}
                <div className="space-y-2">
                  <Label htmlFor="agent">Agent</Label>
                  <Select value={selectedAgentId} onValueChange={setSelectedAgentId}>
                    <SelectTrigger id="agent">
                      <SelectValue placeholder="Select an agent" />
                    </SelectTrigger>
                    <SelectContent>
                      {agents.map((agent: { id: string; name: string; platform: string }) => (
                        <SelectItem key={agent.id} value={agent.id}>
                          <div className="flex items-center gap-2">
                            <IconServer className="h-4 w-4 text-muted-foreground" />
                            <span>{agent.name}</span>
                            <Badge variant="outline" className="ml-1 text-xs capitalize">
                              {agent.platform}
                            </Badge>
                          </div>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">
                    The agent that will analyze this repository.
                  </p>
                </div>
                
                <div className="space-y-2">
                  <Label htmlFor="owner">Owner / Organization</Label>
                  <Input
                    id="owner"
                    placeholder="acme-corp"
                    value={owner}
                    onChange={(e) => setOwner(e.target.value)}
                    required
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="repo">Repository</Label>
                  <Input
                    id="repo"
                    placeholder="api"
                    value={repo}
                    onChange={(e) => setRepo(e.target.value)}
                    required
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="token">
                    GitHub Token <span className="text-muted-foreground">(optional)</span>
                  </Label>
                  <Input
                    id="token"
                    type="password"
                    placeholder="ghp_xxx..."
                    value={token}
                    onChange={(e) => setToken(e.target.value)}
                  />
                  <p className="text-xs text-muted-foreground">
                    Leave blank for demo mode with mock data.
                  </p>
                </div>
                <div className="space-y-2">
                  <Label htmlFor="baseUrl">
                    GitHub Enterprise URL <span className="text-muted-foreground">(optional)</span>
                  </Label>
                  <Input
                    id="baseUrl"
                    placeholder="https://github.mycompany.com"
                    value={baseUrl}
                    onChange={(e) => setBaseUrl(e.target.value)}
                  />
                </div>
                <div className="rounded-lg bg-green-500/10 border border-green-500/20 p-3 flex items-start gap-2">
                  <IconLock className="h-4 w-4 text-green-600 mt-0.5 shrink-0" />
                  <div className="text-sm text-green-700 dark:text-green-400">
                    <strong>Secure by design:</strong> Your GitHub token is stored only in the
                    agent&apos;s vault running in your environment. It never touches our servers.
                  </div>
                </div>
                <div className="flex justify-end gap-2 pt-2">
                  <Button type="button" variant="outline" onClick={() => setAddDialogOpen(false)}>
                    Cancel
                  </Button>
                  <Button type="submit" disabled={addIntegration.isPending || !selectedAgentId}>
                    {addIntegration.isPending ? (
                      <>
                        <IconLoader2 className="mr-2 h-4 w-4 animate-spin" />
                        Adding...
                      </>
                    ) : (
                      "Add Integration"
                    )}
                  </Button>
                </div>
              </form>
            </DialogContent>
          </Dialog>
        </div>
      </div>

      {/* No agents warning */}
      {!loading && hasNoAgents && (
        <Card className="border-yellow-500/30 bg-gradient-to-br from-yellow-500/5 to-transparent">
          <CardContent className="flex items-center gap-4 py-4">
            <div className="flex h-10 w-10 items-center justify-center rounded-full bg-yellow-500/10 ring-1 ring-yellow-500/20 shrink-0">
              <IconAlertCircle className="h-5 w-5 text-yellow-600" />
            </div>
            <div className="flex-1">
              <h3 className="font-semibold text-sm">No agents available</h3>
              <p className="text-sm text-muted-foreground">
                You need to deploy an agent before adding integrations.
              </p>
            </div>
            <Button asChild variant="outline">
              <Link href="/agents?deploy=true">
                <IconServer className="mr-2 h-4 w-4" />
                Deploy Agent
              </Link>
            </Button>
          </CardContent>
        </Card>
      )}

      {/* Integrations list */}
      {loading ? (
        <div className="grid gap-4 md:grid-cols-2">
          {[...Array(2)].map((_, i) => (
            <Card key={i}>
              <CardHeader>
                <div className="flex items-center gap-3">
                  <Skeleton className="h-10 w-10 rounded-full" />
                  <div className="space-y-2">
                    <Skeleton className="h-4 w-32" />
                    <Skeleton className="h-3 w-24" />
                  </div>
                </div>
              </CardHeader>
              <CardContent>
                <Skeleton className="h-9 w-full" />
              </CardContent>
            </Card>
          ))}
        </div>
      ) : integrations.length === 0 ? (
        <Card>
          <CardContent className="flex flex-col items-center justify-center py-16">
            <div className="relative mb-6">
              <div className="absolute inset-0 bg-primary/20 rounded-full blur-xl" />
              <div className="relative flex h-20 w-20 items-center justify-center rounded-full bg-gradient-to-br from-primary/10 to-primary/5 ring-1 ring-primary/20">
                <IconBrandGithub className="h-10 w-10 text-primary" />
              </div>
            </div>
            <h3 className="text-xl font-semibold mb-2">No integrations yet</h3>
            <p className="text-muted-foreground text-center max-w-md mb-6">
              {hasNoAgents 
                ? "Deploy an agent first, then add GitHub integrations to start analyzing."
                : "Add a GitHub integration to start analyzing your code review metrics."
              }
            </p>
            {hasNoAgents ? (
              <Button asChild>
                <Link href="/agents?deploy=true">
                  <IconServer className="mr-2 h-4 w-4" />
                  Deploy Agent
                </Link>
              </Button>
            ) : (
              <Button onClick={() => setAddDialogOpen(true)}>
                <IconPlus className="mr-2 h-4 w-4" />
                Add Integration
              </Button>
            )}
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-4 md:grid-cols-2">
          {integrations.map((integration: Integration) => (
            <Card key={integration.id} className={`relative overflow-hidden ${!integration.isActive ? 'opacity-75 border-yellow-500/30' : ''}`}>
              <div className="absolute top-0 right-0 w-32 h-32 bg-primary/5 rounded-full blur-3xl" />
              <CardHeader>
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <div className={`flex h-10 w-10 items-center justify-center rounded-full ring-1 ${integration.isActive ? 'bg-primary/10 ring-primary/20' : 'bg-yellow-500/10 ring-yellow-500/20'}`}>
                      <IconBrandGithub className={`h-5 w-5 ${integration.isActive ? 'text-primary' : 'text-yellow-600'}`} />
                    </div>
                    <div>
                      <CardTitle className="text-base font-semibold">
                        {integration.owner}/{integration.repo}
                      </CardTitle>
                      <CardDescription className="flex items-center gap-2 mt-1 flex-wrap">
                        {!integration.isActive && (
                          <Badge variant="outline" className="border-yellow-500/50 text-yellow-600 gap-1">
                            <IconAlertTriangle className="h-3 w-3" />
                            Inactive
                          </Badge>
                        )}
                        {integration.hasToken ? (
                          <Badge className="bg-green-500 hover:bg-green-600 gap-1">
                            <IconLock className="h-3 w-3" />
                            Token configured
                          </Badge>
                        ) : (
                          <Badge variant="secondary">Demo mode</Badge>
                        )}
                      </CardDescription>
                    </div>
                  </div>
                </div>
              </CardHeader>
              <CardContent className="space-y-2">
                {!integration.isActive && (
                  <div className="mb-3 rounded-lg bg-yellow-500/10 border border-yellow-500/20 p-3 flex items-start gap-2">
                    <IconAlertTriangle className="h-4 w-4 text-yellow-600 mt-0.5 shrink-0" />
                    <div className="text-xs text-yellow-700 dark:text-yellow-400">
                      The agent for this integration is no longer available. Assign a new agent to continue.
                    </div>
                  </div>
                )}
                <div className="flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1"
                    onClick={() => handleAnalyze(integration)}
                    disabled={!integration.isActive}
                  >
                    <IconRefresh className="mr-2 h-4 w-4" />
                    Analyze
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => handleEditIntegration(integration)}
                  >
                    <IconSettings className="h-4 w-4" />
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Edit Integration Dialog */}
      <Dialog open={editDialogOpen} onOpenChange={setEditDialogOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Edit Integration</DialogTitle>
            <DialogDescription>
              Change the agent for {editingIntegration?.owner}/{editingIntegration?.repo}
            </DialogDescription>
          </DialogHeader>
          <form onSubmit={handleSaveEdit} className="space-y-4">
            {/* Agent Selection */}
            <div className="space-y-2">
              <Label htmlFor="edit-agent">Agent</Label>
              <Select value={editSelectedAgentId} onValueChange={setEditSelectedAgentId}>
                <SelectTrigger id="edit-agent">
                  <SelectValue placeholder="Select an agent" />
                </SelectTrigger>
                <SelectContent>
                  {agents.map((agent: { id: string; name: string; platform: string }) => (
                    <SelectItem key={agent.id} value={agent.id}>
                      <div className="flex items-center gap-2">
                        <IconServer className="h-4 w-4 text-muted-foreground" />
                        <span>{agent.name}</span>
                        <Badge variant="outline" className="ml-1 text-xs capitalize">
                          {agent.platform}
                        </Badge>
                      </div>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                The agent that will analyze this repository.
              </p>
            </div>
            
            {!editingIntegration?.isActive && (
              <div className="rounded-lg bg-yellow-500/10 border border-yellow-500/20 p-3 flex items-start gap-2">
                <IconAlertTriangle className="h-4 w-4 text-yellow-600 mt-0.5 shrink-0" />
                <div className="text-sm text-yellow-700 dark:text-yellow-400">
                  This integration is currently inactive. Assigning a new agent will reactivate it.
                </div>
              </div>
            )}
            
            <div className="flex justify-end gap-2 pt-2">
              <Button type="button" variant="outline" onClick={() => setEditDialogOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" disabled={updateIntegration.isPending || !editSelectedAgentId}>
                {updateIntegration.isPending ? (
                  <>
                    <IconLoader2 className="mr-2 h-4 w-4 animate-spin" />
                    Saving...
                  </>
                ) : (
                  "Save Changes"
                )}
              </Button>
            </div>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  )
}
