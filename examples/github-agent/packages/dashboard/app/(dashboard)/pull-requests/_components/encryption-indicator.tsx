"use client"

import { buttonVariants } from "@/components/ui/button"
import { HoverCard, HoverCardContent, HoverCardTrigger } from "@/components/ui/hover-card"
import { ShineBorder } from "@/components/ui/shine-border"
import { cn } from "@/lib/utils"
import {
  IconExternalLink,
  IconInfoCircle,
  IconLock,
  IconServer,
  IconShieldCheck,
} from "@tabler/icons-react"
import React from "react"

interface EncryptionIndicatorProps {
  agentEnvironment: string
}

export function EncryptionIndicator({ agentEnvironment }: EncryptionIndicatorProps) {
  return (
    <HoverCard openDelay={100} closeDelay={200}>
      <HoverCardTrigger asChild>
        <button
          type="button"
          className="relative overflow-hidden cursor-pointer rounded-md bg-white dark:bg-background/60 backdrop-blur-sm border shadow-sm hover:shadow-md transition-shadow"
        >
          <ShineBorder
            borderWidth={1.5}
            shineColor={["#3AA0FF", "#6366f1", "#22c55e"]}
            duration={10}
          />
          <div className="flex items-center gap-1.5 px-3 py-2 text-xs text-gray-600 dark:text-muted-foreground relative z-10">
            <IconLock className="h-3.5 w-3.5 text-green-600 dark:text-green-500" />
            <span className="whitespace-nowrap font-medium">End-to-end encrypted</span>
            <IconInfoCircle className="h-3.5 w-3.5" />
          </div>
        </button>
      </HoverCardTrigger>
      <HoverCardContent className="w-96 p-0" align="end" side="bottom">
        <div className="p-0.5">
          <div className="rounded-md bg-gradient-to-br from-green-50 via-blue-50 to-purple-50 dark:from-green-500/10 dark:via-blue-500/10 dark:to-purple-500/10 p-4 border border-green-200 dark:border-green-500/20">
            <div className="flex items-center justify-between mb-3">
              <div className="flex items-center gap-2">
                <IconShieldCheck className="h-5 w-5 text-green-600 dark:text-green-500" />
                <h4 className="font-semibold text-gray-900 dark:text-foreground">
                  Private & Secure Data
                </h4>
              </div>
            </div>

            <div className="text-sm mb-3 space-y-2">
              <p className="font-medium text-gray-800 dark:text-foreground">
                Your browser connects directly to your agent:
              </p>
              <ul className="space-y-1.5 text-gray-600 dark:text-muted-foreground ml-2">
                <li className="flex items-start gap-2">
                  <span className="text-green-600">•</span>
                  <span>PR data and AI reviews fetched from your environment</span>
                </li>
                <li className="flex items-start gap-2">
                  <span className="text-blue-600">•</span>
                  <span>Source code and sensitive data never leaves your cloud</span>
                </li>
                <li className="flex items-start gap-2">
                  <span className="text-purple-600">•</span>
                  <span>Only aggregated metrics sent to dashboard</span>
                </li>
              </ul>
            </div>

            <div className="bg-white/80 dark:bg-background/40 px-3 py-2.5 rounded-md border border-gray-200 dark:border-border/50 shadow-sm">
              <div className="flex items-center gap-2 mb-1">
                <IconServer className="h-3.5 w-3.5 text-blue-600 dark:text-blue-500 flex-shrink-0" />
                <p className="text-xs font-semibold text-gray-900 dark:text-foreground">
                  Direct connection to:
                </p>
              </div>
              <p className="text-xs text-gray-600 dark:text-muted-foreground font-mono truncate">
                {agentEnvironment}
              </p>
            </div>

            <div className="mt-3 text-xs flex items-center justify-end">
              <a
                href="https://docs.alien.dev/security"
                target="_blank"
                rel="noopener noreferrer"
                className="text-green-600 dark:text-green-500 hover:text-green-700 dark:hover:text-green-400 flex items-center gap-1 font-medium transition-colors"
              >
                Learn more about security
                <IconExternalLink className="h-3 w-3" />
              </a>
            </div>
          </div>
        </div>
      </HoverCardContent>
    </HoverCard>
  )
}
