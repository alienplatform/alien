"use client"

import { Button } from "@/components/ui/button"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { authClient } from "@/lib/auth-client"
import { IconBrandGithub, IconLoader2 } from "@tabler/icons-react"
import { useRouter } from "next/navigation"
import { useEffect, useState } from "react"
import { toast } from "sonner"

export default function OnboardingPage() {
  const router = useRouter()
  const [organizationName, setOrganizationName] = useState("")
  const [slug, setSlug] = useState("")
  const [isCreating, setIsCreating] = useState(false)
  const [isLoadingSession, setIsLoadingSession] = useState(true)

  useEffect(() => {
    // Check if user already has an organization
    const checkSession = async () => {
      const session = await authClient.getSession()

      if (!session) {
        router.push("/login")
        return
      }

      // Check if user already has an organization
      const orgs = await authClient.organization.listOrganizations()
      if (orgs.data && orgs.data.length > 0) {
        // User already has an organization, redirect to dashboard
        router.push("/")
        return
      }

      setIsLoadingSession(false)
    }

    checkSession()
  }, [router])

  useEffect(() => {
    // Auto-generate slug from organization name
    if (organizationName) {
      const generatedSlug = organizationName
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "")
      setSlug(generatedSlug)
    }
  }, [organizationName])

  const handleCreateOrganization = async () => {
    if (!organizationName.trim()) {
      toast.error("Please enter an organization name")
      return
    }

    if (!slug.trim()) {
      toast.error("Please enter a valid organization slug")
      return
    }

    setIsCreating(true)

    try {
      const result = await authClient.organization.create({
        name: organizationName,
        slug: slug,
      })

      if (result.error) {
        toast.error(result.error.message || "Failed to create organization")
        setIsCreating(false)
        return
      }

      // Set as active organization
      if (result.data?.id) {
        await authClient.organization.setActive({
          organizationId: result.data.id,
        })
      }

      toast.success("Organization created successfully!")

      // Redirect to dashboard
      router.push("/")
    } catch (error) {
      console.error("Failed to create organization:", error)
      toast.error("Failed to create organization")
      setIsCreating(false)
    }
  }

  if (isLoadingSession) {
    return (
      <div className="flex min-h-screen items-center justify-center">
        <IconLoader2 className="h-8 w-8 animate-spin text-muted-foreground" />
      </div>
    )
  }

  return (
    <div className="flex min-h-screen items-center justify-center p-4 bg-gradient-to-br from-background via-background to-primary/5">
      <Card className="w-full max-w-lg">
        <CardHeader className="space-y-4 pb-6">
          <div className="flex justify-center">
            <div className="bg-primary text-primary-foreground flex size-14 items-center justify-center rounded-xl shadow-lg ring-2 ring-primary/20">
              <IconBrandGithub className="size-8" />
            </div>
          </div>
          <div className="space-y-2 text-center">
            <CardTitle className="text-2xl">Welcome to Code Intelligence</CardTitle>
            <CardDescription>Let's get started by creating your organization</CardDescription>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="organization-name">Organization Name</Label>
            <Input
              id="organization-name"
              placeholder="Acme Inc"
              value={organizationName}
              onChange={e => setOrganizationName(e.target.value)}
              disabled={isCreating}
              autoFocus
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="slug">
              Organization Slug
              <span className="text-muted-foreground text-xs ml-2">(used in URLs)</span>
            </Label>
            <Input
              id="slug"
              placeholder="acme-inc"
              value={slug}
              onChange={e => setSlug(e.target.value)}
              disabled={isCreating}
            />
          </div>

          <Button
            className="w-full"
            onClick={handleCreateOrganization}
            disabled={isCreating || !organizationName.trim() || !slug.trim()}
          >
            {isCreating ? (
              <>
                <IconLoader2 className="mr-2 h-4 w-4 animate-spin" />
                Creating Organization...
              </>
            ) : (
              "Create Organization"
            )}
          </Button>
        </CardContent>
      </Card>
    </div>
  )
}
