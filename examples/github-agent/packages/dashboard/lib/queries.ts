/**
 * React Query hooks for data fetching.
 * 
 * These hooks provide type-safe, cached data fetching with automatic
 * refetching, error handling, and loading states.
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query"

/**
 * Query keys for React Query cache management
 */
export const queryKeys = {
  agents: (deploymentGroupId: string) => ["agents", deploymentGroupId] as const,
  agentInfo: (agentId: string) => ["agent", agentId] as const,
  integrations: ["integrations"] as const,
  deploymentGroup: ["deployment-group"] as const,
}

/**
 * List all agents for the current organization
 */
export function useAgents() {
  return useQuery({
    queryKey: ["agents"],
    queryFn: async () => {
      const response = await fetch("/api/agents")
      if (!response.ok) {
        // Return empty array if agents aren't running yet
        return []
      }
      const data = await response.json()
      return data.agents || []
    },
    refetchInterval: 5_000, // Poll every 5 seconds
  })
}

/**
 * Get info for a specific agent
 */
export function useAgentInfo(agentId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.agentInfo(agentId || ""),
    queryFn: async () => {
      if (!agentId) throw new Error("Agent ID is required")
      const response = await fetch(`/api/agents/${agentId}/info`)
      if (!response.ok) throw new Error("Failed to fetch agent info")
      return await response.json()
    },
    enabled: !!agentId,
  })
}

/**
 * Get integrations for the current organization
 */
export function useIntegrations() {
  return useQuery({
    queryKey: queryKeys.integrations,
    queryFn: async () => {
      const response = await fetch("/api/integrations")
      if (!response.ok) throw new Error("Failed to fetch integrations")
      const data = await response.json()
      return data.integrations || []
    },
  })
}

/**
 * Get or create deployment group for the current organization
 */
export function useDeploymentGroup() {
  return useQuery({
    queryKey: queryKeys.deploymentGroup,
    queryFn: async () => {
      const response = await fetch("/api/organizations/deployment-group")
      if (!response.ok) {
        const error = await response.text()
        throw new Error(error || "Failed to get deployment group")
      }
      return await response.json()
    },
    retry: 1,
  })
}

/**
 * Add a new integration
 */
export function useAddIntegration() {
  const queryClient = useQueryClient()
  
  return useMutation({
    mutationFn: async (data: {
      owner: string
      repo: string
      token?: string
      baseUrl?: string
      agentId: string
    }) => {
      const response = await fetch("/api/integrations", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
      })
      
      if (!response.ok) {
        const error = await response.json()
        throw new Error(error.error || "Failed to add integration")
      }
      
      return await response.json()
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.integrations })
    },
  })
}

/**
 * Analyze an integration
 */
export function useAnalyzeIntegration() {
  return useMutation({
    mutationFn: async (integrationId: string) => {
      const response = await fetch("/api/analyze", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ integrationId }),
      })
      
      if (!response.ok) throw new Error("Analysis failed")
      return await response.json()
    },
  })
}

/**
 * Update an integration (e.g., reassign agent)
 */
export function useUpdateIntegration() {
  const queryClient = useQueryClient()
  
  return useMutation({
    mutationFn: async (data: {
      integrationId: string
      agentId?: string
      isActive?: boolean
    }) => {
      const response = await fetch(`/api/integrations/${data.integrationId}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
      })
      
      if (!response.ok) {
        const error = await response.json()
        throw new Error(error.error || "Failed to update integration")
      }
      
      return await response.json()
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.integrations })
    },
  })
}


