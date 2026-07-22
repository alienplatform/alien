"use client"

import { useEffect, useState } from "react"

// Terminal-style braille spinner, the in-progress marker across Alien surfaces.
const FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]

export function Spinner({ className }: { className?: string }) {
  const [frame, setFrame] = useState(0)
  useEffect(() => {
    const timer = setInterval(() => setFrame(f => (f + 1) % FRAMES.length), 80)
    return () => clearInterval(timer)
  }, [])
  return (
    <span className={`font-mono ${className ?? "text-yellow-400"}`} aria-hidden="true">
      {FRAMES[frame]}
    </span>
  )
}
