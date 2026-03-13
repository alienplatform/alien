"use client"

import { cn } from "@/lib/utils"
import { IconCircleCheck } from "@tabler/icons-react"
import Image from "next/image"

export type Platform = "aws" | "gcp" | "azure" | "kubernetes" | "local"

interface PlatformInfo {
  name: string
  description: string
  icon: string
}

// Platform order: aws, gcp, azure, kubernetes, local (local is last)
const platformOrder: Platform[] = ["aws", "gcp", "azure", "kubernetes", "local"]

const platformInfo: Record<Platform, PlatformInfo> = {
  aws: {
    name: "AWS",
    description: "Deploy to AWS account",
    icon: "/aws.svg",
  },
  gcp: {
    name: "Google Cloud",
    description: "Deploy to GCP project",
    icon: "/google-cloud.svg",
  },
  azure: {
    name: "Azure",
    description: "via Azure Lighthouse",
    icon: "/azure.svg",
  },
  kubernetes: {
    name: "Kubernetes",
    description: "Deploy to your cluster",
    icon: "/kubernetes.svg",
  },
  local: {
    name: "Local",
    description: "Run on your machine",
    icon: "/local.svg",
  },
}

interface PlatformSelectorProps {
  selected: Platform
  onSelect: (platform: Platform) => void
}

export function PlatformSelector({ selected, onSelect }: PlatformSelectorProps) {
  return (
    <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
      {platformOrder.map(platform => {
        const info = platformInfo[platform]
        const isSelected = selected === platform

        return (
          <button
            key={platform}
            type="button"
            onClick={() => onSelect(platform)}
            className={cn(
              "group relative flex flex-col items-center justify-center py-4 rounded-lg border transition-all duration-200",
              isSelected
                ? "border-primary/30 bg-primary/5 ring-2 ring-primary/20 shadow-sm"
                : "border-border/50 hover:bg-accent/50 hover:shadow-sm hover:border-primary/20",
            )}
          >
            {isSelected && (
              <div className="absolute -top-1.5 -right-1.5 p-0.5 rounded-full bg-primary text-primary-foreground shadow-sm">
                <IconCircleCheck className="h-3.5 w-3.5" />
              </div>
            )}
            <div className="w-10 h-10 mb-2 relative transition-transform group-hover:scale-105">
              <Image
                src={info.icon}
                alt={info.name}
                fill
                className={cn(
                  "object-contain",
                  // Add brightness filter for AWS and Local in light mode
                  platform === "local" && "dark:brightness-100 brightness-0",
                )}
              />
            </div>
            <span className="text-sm font-medium">{info.name}</span>
            <span className="text-xs text-muted-foreground/70 mt-0.5 line-clamp-1 text-center">
              {info.description}
            </span>
          </button>
        )
      })}
    </div>
  )
}
