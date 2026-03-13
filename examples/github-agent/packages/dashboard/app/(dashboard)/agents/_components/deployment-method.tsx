"use client"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  IconArrowRight,
  IconCheck,
  IconChevronDown,
  IconCircleCheck,
  IconDownload,
  IconExternalLink,
} from "@tabler/icons-react"
import Image from "next/image"
import { useEffect, useMemo, useState } from "react"
import { CodeDisplay } from "./code-display"
import type { Platform } from "./platform-selector"

type OS = "windows" | "mac" | "linux"
type DeploymentMethod = OS | "cli" | "cloudformation" | "terraform" | "google-login" | "dev"

interface DeploymentMethodProps {
  platform: Platform
  token?: string
  projectName?: string
}

function detectOS(): OS {
  if (typeof window === "undefined") return "linux"
  const ua = window.navigator.userAgent.toLowerCase()
  if (ua.includes("win")) return "windows"
  if (ua.includes("mac")) return "mac"
  return "linux"
}

const methodLabels: Record<DeploymentMethod, string> = {
  windows: "Windows",
  mac: "Mac",
  linux: "Linux",
  cli: "CLI",
  cloudformation: "CloudFormation",
  terraform: "Terraform",
  "google-login": "Google Login",
  dev: "Local Development",
}

export function DeploymentMethod({
  platform,
  token,
  projectName = "github-agent",
}: DeploymentMethodProps) {
  const detectedOS = useMemo(() => detectOS(), [])
  const isLocal = platform === "local"

  const getAvailableMethods = (): DeploymentMethod[] => {
    if (isLocal) {
      return ["windows", "mac", "linux", "dev"]
    }

    const methods: DeploymentMethod[] = []

    // GCP: Google Login, Terraform, CLI
    if (platform === "gcp") {
      methods.push("google-login")
    }

    // AWS: CloudFormation, Terraform, CLI
    if (platform === "aws") {
      methods.push("cloudformation")
    }

    // Terraform available for AWS, GCP, Azure, and Kubernetes
    if (["aws", "gcp", "azure", "kubernetes"].includes(platform)) {
      methods.push("terraform")
    }

    // CLI available for all non-local platforms
    methods.push("cli")

    return methods
  }

  const getDefaultMethod = (): DeploymentMethod => {
    if (isLocal) return detectedOS
    if (platform === "gcp") return "google-login"
    if (platform === "aws") return "cloudformation"
    if (platform === "azure" || platform === "kubernetes") return "terraform"
    return "cli"
  }

  const [selectedMethod, setSelectedMethod] = useState<DeploymentMethod>(getDefaultMethod)

  // Update selected method when platform changes
  useEffect(() => {
    let newMethod: DeploymentMethod
    if (isLocal) {
      newMethod = detectedOS
    } else if (platform === "gcp") {
      newMethod = "google-login"
    } else if (platform === "aws") {
      newMethod = "cloudformation"
    } else if (platform === "azure" || platform === "kubernetes") {
      newMethod = "terraform"
    } else {
      newMethod = "cli"
    }
    setSelectedMethod(newMethod)
  }, [platform, isLocal, detectedOS])

  const getInstallCommand = (os: OS) => {
    const baseUrl = "https://get.alien.dev"

    if (os === "windows") {
      return `# Download and run the installer
Invoke-WebRequest -Uri "${baseUrl}/install.ps1" -OutFile "install.ps1"
.\\install.ps1`
    }

    return `# Install via curl
curl -sSL ${baseUrl}/install.sh | bash`
  }

  const devCommand = `alien dev --token ${token || "dg_xxx..."}`
  const cliCommand = `alien deploy --platform=${platform} --token=${token || "dg_xxx..."}`

  const availableMethods = getAvailableMethods()

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-1 flex-wrap">
        <h3 className="text-base font-semibold">{isLocal ? "Deploy on" : "Deploy via"}</h3>
        <DropdownMenu>
          <DropdownMenuTrigger className="inline-flex items-center gap-1 text-base font-medium text-primary hover:underline data-[state=open]:underline underline-offset-4 focus:outline-none">
            {methodLabels[selectedMethod]}
            <IconChevronDown className="h-3.5 w-3.5" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-44">
            {availableMethods.map(method => (
              <DropdownMenuItem
                key={method}
                onClick={() => setSelectedMethod(method)}
                className="flex items-center justify-between cursor-pointer text-sm"
              >
                <span>{methodLabels[method]}</span>
                {selectedMethod === method && <IconCheck className="h-3.5 w-3.5 text-primary" />}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="space-y-3">
        {/* Local Development */}
        {isLocal && selectedMethod === "dev" && (
          <div className="space-y-4">
            <div className="space-y-2.5">
              <h4 className="text-sm font-semibold">Run the agent locally</h4>
              <CodeDisplay code={devCommand} />
            </div>

            <InfoBox>
              <h4 className="text-sm font-semibold mb-2.5">What happens next?</h4>
              <ul className="text-xs text-muted-foreground space-y-1.5">
                <ListItem>The dev server starts on port 9090</ListItem>
                <ListItem>Agent builds and runs locally</ListItem>
                <ListItem>Agent registers and appears in this dashboard</ListItem>
              </ul>
            </InfoBox>
          </div>
        )}

        {/* Local Platform - OS-specific instructions */}
        {isLocal &&
          (selectedMethod === "windows" ||
            selectedMethod === "mac" ||
            selectedMethod === "linux") && (
            <div className="space-y-4">
              <div className="space-y-2.5">
                <h4 className="text-sm font-semibold">Install the CLI</h4>
                <CodeDisplay
                  language={selectedMethod === "windows" ? "powershell" : "bash"}
                  code={getInstallCommand(selectedMethod)}
                />
              </div>

              <div className="space-y-2.5">
                <h4 className="text-sm font-semibold">Run the Application</h4>
                <CodeDisplay code={`${projectName} install --token=${token || "dg_xxx..."}`} />
              </div>

              <InfoBox>
                <h4 className="text-sm font-semibold mb-2.5">What happens next?</h4>
                <ul className="text-xs text-muted-foreground space-y-1.5">
                  <ListItem>The CLI authenticates using the deployment token</ListItem>
                  <ListItem>Required dependencies are downloaded</ListItem>
                  <ListItem>Your agent starts running locally</ListItem>
                  <ListItem>Real-time logs appear in your terminal</ListItem>
                </ul>
              </InfoBox>
            </div>
          )}

        {/* CLI for non-local platforms */}
        {!isLocal && selectedMethod === "cli" && (
          <div className="space-y-4">
            <div className="space-y-2.5">
              <h4 className="text-sm font-semibold">Install the CLI</h4>
              <CodeDisplay
                language={detectedOS === "windows" ? "powershell" : "bash"}
                code={getInstallCommand(detectedOS)}
              />
            </div>

            <div className="space-y-2.5">
              <h4 className="text-sm font-semibold">Deploy to {platform.toUpperCase()}</h4>
              <CodeDisplay code={cliCommand} />
            </div>

            <InfoBox>
              <h4 className="text-sm font-semibold mb-2.5">What happens next?</h4>
              <ul className="text-xs text-muted-foreground space-y-1.5">
                <ListItem>The CLI authenticates using the deployment token</ListItem>
                <ListItem>Required cloud resources are provisioned</ListItem>
                <ListItem>Your agent is deployed to {platform.toUpperCase()}</ListItem>
                <ListItem>Real-time deployment status appears in your terminal</ListItem>
              </ul>
            </InfoBox>
          </div>
        )}

        {/* Google Login */}
        {selectedMethod === "google-login" && (
          <div className="space-y-4">
            <p className="text-sm text-muted-foreground/80">
              Click the button below to sign in with your Google account. Provisioning will happen
              automatically after authentication.
            </p>

            <Button variant="outline" className="rounded-full gap-x-2 px-6 py-6 w-full font-medium">
              <Image src="/google-cloud.svg" alt="Google Cloud" width={20} height={20} />
              <span>Sign in with Google</span>
            </Button>

            <InfoBox>
              <h4 className="text-sm font-semibold mb-2.5">What happens next?</h4>
              <ul className="text-xs text-muted-foreground space-y-1.5">
                <ListItem>You'll be redirected to Google's authentication page</ListItem>
                <ListItem>Grant permissions to access your Google Cloud account</ListItem>
                <ListItem>Cloud resources are provisioned automatically</ListItem>
                <ListItem>Your agent is deployed to Google Cloud</ListItem>
              </ul>
            </InfoBox>
          </div>
        )}

        {/* CloudFormation */}
        {selectedMethod === "cloudformation" && (
          <div className="space-y-4">
            <p className="text-sm text-muted-foreground/80">
              Click the button below to deploy using AWS CloudFormation. This will open the AWS
              Console with a pre-configured template.
            </p>

            <Button variant="outline" className="rounded-full gap-x-2 px-6 py-6 w-full font-medium">
              <Image src="/aws.svg" alt="AWS" width={20} height={20} />
              <span>Deploy with CloudFormation</span>
            </Button>

            <InfoBox>
              <h4 className="text-sm font-semibold mb-2.5">Prerequisites</h4>
              <ul className="text-xs text-muted-foreground space-y-1.5">
                <ListItem>You must be logged into your AWS account</ListItem>
                <ListItem>You need permissions to create CloudFormation stacks</ListItem>
                <ListItem>Required IAM permissions will be created automatically</ListItem>
              </ul>
            </InfoBox>
          </div>
        )}

        {/* Terraform */}
        {selectedMethod === "terraform" && (
          <div className="space-y-4">
            <div className="space-y-2.5">
              <h4 className="text-sm font-semibold">Configure Terraform Provider</h4>
              <CodeDisplay
                language="hcl"
                code={`terraform {
  required_providers {
    ${projectName.replace(/-/g, "_")} = {
      source  = "registry.terraform.io/alien-dev/${projectName}"
      version = "~> 1.0"
    }
  }
}

provider "${projectName.replace(/-/g, "_")}" {
  agent_key = "${token}"
}

resource "${projectName.replace(/-/g, "_")}_agent" "main" {
  platform = "${platform}"
}`}
              />
            </div>

            <div className="flex gap-2.5">
              <Button variant="outline" size="sm" className="flex-1" disabled>
                <IconDownload className="h-3.5 w-3.5 mr-1.5" />
                <span className="text-xs font-medium">Download Example</span>
              </Button>
              <Button variant="outline" size="sm" className="flex-1" asChild>
                <a
                  href={`https://registry.terraform.io/providers/alien-dev/${projectName}/latest/docs`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="flex items-center justify-center gap-1.5"
                >
                  <IconExternalLink className="h-3.5 w-3.5" />
                  <span className="text-xs font-medium">View Documentation</span>
                </a>
              </Button>
            </div>

            <InfoBox>
              <h4 className="text-sm font-semibold mb-2.5">Getting Started</h4>
              <ol className="text-xs text-muted-foreground space-y-1.5">
                <NumberedItem n={1}>
                  Save the configuration above as{" "}
                  <code className="text-xs bg-background px-1.5 py-0.5 rounded border">
                    main.tf
                  </code>
                </NumberedItem>
                <NumberedItem n={2}>
                  Run{" "}
                  <code className="text-xs bg-background px-1.5 py-0.5 rounded border">
                    terraform init
                  </code>{" "}
                  to download the provider
                </NumberedItem>
                <NumberedItem n={3}>
                  Run{" "}
                  <code className="text-xs bg-background px-1.5 py-0.5 rounded border">
                    terraform plan
                  </code>{" "}
                  to preview
                </NumberedItem>
                <NumberedItem n={4}>
                  Run{" "}
                  <code className="text-xs bg-background px-1.5 py-0.5 rounded border">
                    terraform apply
                  </code>{" "}
                  to deploy
                </NumberedItem>
              </ol>
            </InfoBox>
          </div>
        )}
      </div>
    </div>
  )
}

function InfoBox({ children }: { children: React.ReactNode }) {
  return (
    <div className="bg-primary/5 rounded-lg p-4 border border-primary/10">
      <div className="flex items-start gap-3">
        <div className="p-1.5 rounded-lg bg-primary/10 ring-1 ring-primary/20 mt-0.5 flex-shrink-0">
          <IconCircleCheck className="h-4 w-4 text-primary" />
        </div>
        <div className="flex-1">{children}</div>
      </div>
    </div>
  )
}

function ListItem({ children }: { children: React.ReactNode }) {
  return (
    <li className="flex items-start gap-1.5">
      <IconArrowRight className="h-3.5 w-3.5 mt-0.5 flex-shrink-0 text-primary" />
      <span>{children}</span>
    </li>
  )
}

function NumberedItem({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <li className="flex items-start gap-1.5">
      <span className="font-bold mt-0.5 flex-shrink-0 w-4 text-primary">{n}.</span>
      <span>{children}</span>
    </li>
  )
}
