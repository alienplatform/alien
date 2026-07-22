"use client"

import dynamic from "next/dynamic"

// Alien-brand hero background: a green grain-gradient shader over near-black,
// with a radial-gradient fallback while the shader loads (or without WebGL).
const GrainGradient = dynamic(
  () => import("@paper-design/shaders-react").then(mod => mod.GrainGradient),
  { ssr: false, loading: () => <Fallback /> },
)

export function GrainBackground() {
  return (
    <div aria-hidden className="absolute inset-0 overflow-hidden">
      <Fallback />
      <GrainGradient
        className="absolute inset-0"
        colors={["#1a8a2e", "#064e1a", "#0a2e1200"]}
        colorBack="#00000000"
        softness={2}
        intensity={0.3}
        noise={0.12}
        speed={0.2}
        shape="corners"
        minPixelRatio={1}
        maxPixelCount={1920 * 1080}
      />
    </div>
  )
}

function Fallback() {
  return (
    <div
      aria-hidden
      className="absolute inset-0 bg-[radial-gradient(85%_70%_at_8%_10%,rgba(34,197,94,0.10),transparent_62%),radial-gradient(80%_75%_at_92%_0%,rgba(45,212,160,0.06),transparent_58%)]"
    />
  )
}
