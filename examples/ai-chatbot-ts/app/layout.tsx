import { GeistMono } from "geist/font/mono"
import { GeistSans } from "geist/font/sans"
import type { Metadata } from "next"
import type { ReactNode } from "react"
import "./globals.css"

export const metadata: Metadata = {
  title: "AI chatbot on Alien",
  description: "A streaming chatbot that talks to cloud LLMs through the Alien AI gateway.",
}

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`${GeistSans.variable} ${GeistMono.variable}`}>
      <body className="bg-zinc-950 font-sans text-white antialiased">{children}</body>
    </html>
  )
}
