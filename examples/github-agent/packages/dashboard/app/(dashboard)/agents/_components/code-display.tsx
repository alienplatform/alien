"use client"

import { Button } from "@/components/ui/button"
import { ScrollArea, ScrollBar } from "@/components/ui/scroll-area"
import { IconCheck, IconCopy } from "@tabler/icons-react"
import { useState } from "react"
import { toast } from "sonner"

interface CodeDisplayProps {
  code: string
  language?: string
}

export function CodeDisplay({ code, language = "bash" }: CodeDisplayProps) {
  const [copied, setCopied] = useState(false)

  const handleCopy = () => {
    navigator.clipboard.writeText(code)
    setCopied(true)
    toast.success("Copied to clipboard!")
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <ScrollArea className="border rounded-lg p-3 bg-card/50">
      <div className="group relative">
        <Button
          variant="ghost"
          size="icon"
          className="absolute right-0 top-0 h-7 w-7 opacity-0 group-hover:opacity-100 transition-opacity"
          onClick={handleCopy}
        >
          {copied ? (
            <IconCheck className="h-3.5 w-3.5 text-green-500" />
          ) : (
            <IconCopy className="h-3.5 w-3.5" />
          )}
        </Button>
        <pre className="text-xs font-mono overflow-x-auto pr-8">
          <code className={`language-${language}`}>{code}</code>
        </pre>
      </div>
      <ScrollBar orientation="horizontal" forceMount />
    </ScrollArea>
  )
}
