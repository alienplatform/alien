"use client"

import type { UIMessage } from "ai"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"
import { QueryCard, type QueryInput, type QueryOutput } from "./query-card"

// The generic UIMessage part type doesn't know this app's tool shapes; narrow
// the queryDatabase parts to what the server tool actually sends.
type QueryToolPart = {
  type: "tool-queryDatabase"
  toolCallId: string
  state: "input-streaming" | "input-available" | "output-available" | "output-error"
  input?: QueryInput
  output?: QueryOutput
  errorText?: string
}

export function Message({ message }: { message: UIMessage }) {
  if (message.role === "user") {
    const text = message.parts.map(part => (part.type === "text" ? part.text : "")).join("")
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%] rounded-2xl rounded-br-md border border-white/40 bg-zinc-800 px-4 py-2.5 text-white">
          {text}
        </div>
      </div>
    )
  }

  return (
    <div className="space-y-1">
      {message.parts.map((part, i) => {
        const key = `${message.id}-${i}`
        if (part.type === "text") {
          return (
            <div
              key={key}
              className="prose prose-invert max-w-none rounded-xl border border-white/40 bg-card/80 px-4 py-3 text-zinc-50 backdrop-blur-sm [--tw-prose-invert-body:var(--color-zinc-50)] [--tw-prose-invert-bullets:var(--color-zinc-400)] [--tw-prose-invert-counters:var(--color-zinc-400)] prose-a:text-brand prose-strong:text-white"
            >
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{part.text}</ReactMarkdown>
            </div>
          )
        }
        if (part.type === "tool-queryDatabase") {
          const tool = part as unknown as QueryToolPart
          return (
            <QueryCard
              key={tool.toolCallId}
              state={tool.state}
              input={tool.input}
              output={tool.output}
              errorText={tool.errorText}
            />
          )
        }
        return null
      })}
    </div>
  )
}
