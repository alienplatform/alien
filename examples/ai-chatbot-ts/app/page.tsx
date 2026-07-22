"use client"

import { useChat } from "@ai-sdk/react"
import { useEffect, useRef, useState } from "react"
import { GrainBackground } from "./components/grain-background"
import { Message } from "./components/message"
import { Spinner } from "./components/spinner"

// Matched to the /api/seed dataset so first-run questions land.
const SUGGESTIONS = [
  "How many enterprise customers do we have and what's their total MRR?",
  "Who are our top 5 customers by MRR?",
  "How many orders are pending, and what are they worth?",
  "Break down our customers by country.",
]

export default function Chat() {
  const [input, setInput] = useState("")
  const [models, setModels] = useState<string[]>([])
  const [model, setModel] = useState("")
  const [seedNote, setSeedNote] = useState("")
  const { messages, sendMessage, status, stop, error, regenerate } = useChat()
  const bottomRef = useRef<HTMLDivElement>(null)
  const composerRef = useRef<HTMLTextAreaElement>(null)

  const busy = status === "submitted" || status === "streaming"

  // Model ids come from the cloud's catalog, not hardcoded.
  useEffect(() => {
    fetch("/api/models")
      .then(r => r.json())
      .then((d: { models: string[] }) => {
        setModels(d.models)
        if (d.models[0]) setModel(d.models[0])
      })
      .catch(() => {})
  }, [])

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll on every stream update
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" })
  }, [messages])

  function ask(text: string) {
    sendMessage({ text }, { body: { model } })
    composerRef.current?.focus()
  }

  function submit() {
    const text = input.trim()
    if (!text || busy) return
    ask(text)
    setInput("")
  }

  async function seed() {
    try {
      const r = await fetch("/api/seed", { method: "POST" })
      const d: { customers?: number } = await r.json()
      setSeedNote(r.ok ? `✓ Seeded ${d.customers} customers` : "Seeding failed")
    } catch {
      setSeedNote("Seeding failed")
    }
    setTimeout(() => setSeedNote(""), 4000)
  }

  return (
    <div className="relative flex h-dvh flex-col">
      <GrainBackground />
      <header className="relative flex items-center gap-2.5 border-b border-edge px-4 py-3">
        <span className="size-2 rounded-full bg-brand" aria-hidden="true" />
        <h1 className="text-sm font-medium tracking-tight">AI chatbot on Alien</h1>
        <div className="ml-auto flex items-center gap-2">
          {seedNote && (
            <span
              className={`font-mono text-xs ${seedNote.startsWith("✓") ? "text-brand" : "text-red-400"}`}
            >
              {seedNote}
            </span>
          )}
          <button
            type="button"
            onClick={seed}
            className="cursor-pointer rounded-full border border-edge px-3 py-1.5 text-xs font-medium text-zinc-100 transition-colors hover:bg-white/5 hover:text-white outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
          >
            Seed demo data
          </button>
          <select
            value={model}
            onChange={e => setModel(e.target.value)}
            className="cursor-pointer rounded-full border border-edge bg-transparent px-3 py-1.5 font-mono text-xs text-zinc-100 outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
          >
            {models.map(m => (
              <option key={m} value={m} className="bg-zinc-950">
                {m}
              </option>
            ))}
          </select>
        </div>
      </header>

      <main className="relative flex-1 overflow-y-auto">
        <div className="mx-auto max-w-3xl px-4 py-6">
          {messages.length === 0 ? (
            <div className="flex flex-col items-center gap-8 pt-24 text-center text-landing-foreground">
              <div className="flex flex-col items-center">
                <div className="flex w-fit items-center gap-2 rounded-full border border-brand/50 px-3 py-1.5 font-mono text-xs text-brand">
                  <span className="font-semibold">LIVE</span>
                  <span>AI connected to a private Postgres</span>
                </div>
                <h2 className="mt-6 text-2xl font-medium leading-tight tracking-tight md:text-3xl">
                  Ask your data <span className="text-brand">anything.</span>
                </h2>
                <p className="mt-3 text-sm leading-relaxed text-zinc-300">
                  Answers come from live SQL against the stack's private Postgres, with no
                  credentials in the app.
                </p>
              </div>
              <div className="grid w-full gap-2 sm:grid-cols-2">
                {SUGGESTIONS.map(suggestion => (
                  <button
                    key={suggestion}
                    type="button"
                    onClick={() => ask(suggestion)}
                    className="cursor-pointer rounded-xl border border-white/40 bg-card/70 px-4 py-3 text-left text-sm text-zinc-200 backdrop-blur-sm transition-colors hover:border-brand/60 hover:text-white outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
                  >
                    {suggestion}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="space-y-6">
              {messages.map(message => (
                <Message key={message.id} message={message} />
              ))}
              {status === "submitted" && (
                <div className="flex items-center gap-2 font-mono text-xs text-zinc-400">
                  <Spinner className="text-yellow-400" />
                  Thinking
                </div>
              )}
              {error && (
                <div className="rounded-xl border border-red-500/30 bg-red-500/10 px-4 py-3 text-sm text-red-400">
                  Something went wrong.{" "}
                  <button
                    type="button"
                    onClick={() => regenerate()}
                    className="cursor-pointer font-medium underline"
                  >
                    Retry
                  </button>
                </div>
              )}
            </div>
          )}
          <div ref={bottomRef} />
        </div>
      </main>

      <footer className="relative px-4 pb-4">
        <form
          className="relative mx-auto max-w-3xl"
          onSubmit={e => {
            e.preventDefault()
            submit()
          }}
        >
          <textarea
            ref={composerRef}
            value={input}
            rows={1}
            // biome-ignore lint/a11y/noAutofocus: a chat app's single input is the page's purpose
            autoFocus
            placeholder="Ask about the company's data…"
            onChange={e => setInput(e.currentTarget.value)}
            onKeyDown={e => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault()
                submit()
              }
            }}
            className="block max-h-40 w-full resize-none rounded-xl border border-white/40 bg-card/80 py-3 pl-4 pr-12 outline-none backdrop-blur-sm field-sizing-content caret-white placeholder:text-zinc-400 focus:border-white/70 focus:ring-2 focus:ring-white/25"
          />
          {busy ? (
            <button
              type="button"
              onClick={() => stop()}
              aria-label="Stop"
              className="absolute bottom-2 right-2 flex size-8 cursor-pointer items-center justify-center rounded-full bg-brand text-brand-foreground shadow-lg shadow-green-950/30 transition-colors hover:bg-brand-200 outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
            >
              <StopIcon />
            </button>
          ) : (
            <button
              type="submit"
              aria-label="Send"
              disabled={!input.trim()}
              className="absolute bottom-2 right-2 flex size-8 cursor-pointer items-center justify-center rounded-full bg-brand text-brand-foreground shadow-lg shadow-green-950/30 transition-colors hover:bg-brand-200 disabled:cursor-default disabled:opacity-30 outline-none focus-visible:ring-2 focus-visible:ring-brand/50"
            >
              <ArrowUpIcon />
            </button>
          )}
        </form>
      </footer>
    </div>
  )
}

function ArrowUpIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 19V5" />
      <path d="m5 12 7-7 7 7" />
    </svg>
  )
}

function StopIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true">
      <rect width="12" height="12" rx="2" />
    </svg>
  )
}
